//! MIR: each function body as a control-flow graph of basic blocks over
//! explicit places, with moves, borrows and drops spelled out (spec
//! Appendix E). The interpreter (`crate::interp`) runs it; later milestones
//! borrow-check it (M8) and generate LLVM IR from it (M7).

mod borrowck;
mod build;
mod elaborate;
mod mono;

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

pub use borrowck::check as borrow_check;
pub use build::{build_program, needs_drop_ty, op_ty as build_op_ty};
pub use elaborate::elaborate_drops;
pub use mono::check as mono_check;

use crate::ast::{BinOp, UnOp};
use crate::resolve::{DefId, FileId};
use crate::source::Span;
use crate::typeck::results::AdtRef;
use crate::typeck::FnId;
use crate::types::{IntTy, Ty};

pub type Local = u32;
pub type BlockId = u32;
pub type BodyId = u32;

pub const RETURN: Local = 0;

#[derive(Default)]
pub struct Program {
    pub bodies: Vec<Body>,
    pub fn_bodies: HashMap<FnId, BodyId>,
    pub const_bodies: HashMap<DefId, BodyId>,
    /// Errors found while lowering (moves out of closure captures).
    pub errors: Vec<(FileId, crate::diag::Diagnostic)>,
}

#[derive(Clone, Debug)]
pub enum BodyKind {
    Fn(FnId),
    /// A closure; its first `captures` parameters are the captured values.
    Closure {
        captures: u32,
    },
    Const(DefId),
}

/// The code behind a trait method for concrete types (§13.3).
#[derive(Clone, Debug)]
pub enum Resolved {
    /// A body (an implementation's method, or the trait's default) and its types.
    Body(BodyId, Vec<Ty>),
    /// A built-in implementation, named `Trait.method`.
    Native(String),
}

/// The trait and method name a trait-method function belongs to.
pub fn trait_method_of(g: &crate::typeck::Globals, fid: FnId) -> Option<(DefId, String)> {
    g.traits
        .iter()
        .find_map(|(&def, tr)| tr.methods.iter().find(|(_, f)| *f == fid).map(|(n, _)| (def, n.clone())))
}

/// Resolves a trait method call with concrete types: `tys` is
/// `[idhi, trait args…, own generics…]`.
pub fn resolve_trait(prog: &Program, g: &crate::typeck::Globals, fid: FnId, tys: &[Ty]) -> Option<Resolved> {
    let (tdef, name) = trait_method_of(g, fid)?;
    let tr = &g.traits[&tdef];
    let ntrait = tr.generics.len();
    let self_ty = tys.first().cloned().unwrap_or(Ty::Error);
    let trait_args: Vec<Ty> = tys.iter().skip(1).take(ntrait.saturating_sub(1)).cloned().collect();
    let own: Vec<Ty> = tys.iter().skip(ntrait).cloned().collect();
    let solver = crate::typeck::solve::Solver { g, generics: &[] };
    let mut table = crate::types::InferTable::default();
    let tref = crate::typeck::TraitRef { def: tdef, args: trait_args };
    Some(match solver.find_impl(&mut table, &self_ty, &tref, 0) {
        Some((idx, fresh)) => {
            let imp = &g.impls[idx];
            let impl_args: Vec<Ty> = fresh.iter().map(|t| table.resolve(t)).collect();
            match imp.methods.get(&name).and_then(|m| prog.fn_bodies.get(m)) {
                Some(&body) => {
                    let mut all = impl_args;
                    all.extend(own);
                    Resolved::Body(body, all)
                }
                None => match prog.fn_bodies.get(&fid) {
                    Some(&body) => Resolved::Body(body, tys.to_vec()),
                    None => Resolved::Native(format!("{}.{name}", tr.name)),
                },
            }
        }
        None => Resolved::Native(format!("{}.{name}", tr.name)),
    })
}

pub struct Body {
    pub name: String,
    pub kind: BodyKind,
    pub file: Option<FileId>,
    /// Local 0 is the return value; then the parameters (for a closure, its
    /// captures first), then everything else.
    pub locals: Vec<LocalDecl>,
    pub params: u32,
    pub blocks: Vec<Block>,
}

pub struct LocalDecl {
    pub ty: Ty,
    pub name: Option<String>,
}

pub struct Block {
    pub stmts: Vec<Stmt>,
    /// The source span of each statement, for diagnostics.
    pub spans: Vec<Span>,
    pub term: Term,
    pub term_span: Span,
}

impl Block {
    pub fn new() -> Self {
        Block { stmts: Vec::new(), spans: Vec::new(), term: Term::Unreachable, term_span: Span::default() }
    }
}

impl Default for Block {
    fn default() -> Self {
        Block::new()
    }
}

pub enum Stmt {
    Assign(Place, Rvalue),
    /// Writes a value whose old contents must be dropped first (§17.2).
    Replace(Place, Rvalue, Ty),
    /// Runs the destructor of the value at the place, if it is initialized
    /// (§19.3), and leaves the place uninitialized.
    Drop(Place, Ty),
    /// The local's scope ends: references to it must be dead by now (§15.4.5).
    StorageDead(Local),
    /// Frees a `pette`'s memory without dropping its contents, which were
    /// moved out (`*b`).
    FreeBox(Place, Ty),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub local: Local,
    pub proj: Vec<Proj>,
}

impl Place {
    pub fn local(local: Local) -> Self {
        Place { local, proj: Vec::new() }
    }

    pub fn project(&self, p: Proj) -> Self {
        let mut proj = self.proj.clone();
        proj.push(p);
        Place { local: self.local, proj }
    }

    pub fn field(&self, i: u32) -> Self {
        self.project(Proj::Field(i))
    }

    pub fn deref(&self) -> Self {
        self.project(Proj::Deref)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Proj {
    /// Through a reference or a `pette`.
    Deref,
    /// A record, tuple or variant field.
    Field(u32),
    /// Views an enum value as one of its variants, before its fields.
    Variant(u32),
    /// An element of an array, `varusa` or `mukka`; traps T03 when out of bounds.
    Index(Local, Span),
}

#[derive(Clone, Debug)]
pub enum Operand {
    Copy(Place),
    Move(Place),
    Const(Const),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Const {
    Unit,
    Bool(bool),
    Int(i128),
    F32(f32),
    F64(f64),
    Char(char),
    Str(Rc<str>),
    Fn(FnId, Vec<Ty>),
}

/// The operand type of a built-in operator.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OpTy {
    Int(IntTy),
    Float(u8),
    Bool,
    Char,
    Str,
    Unit,
}

#[derive(Clone, Debug)]
pub enum Agg {
    Tuple,
    Array,
    Varusa,
    /// Operands alternate key, value.
    Jodi,
    Adt(AdtRef, u32),
    Pette,
    Closure(BodyId),
}

pub enum Rvalue {
    Use(Operand),
    Ref(bool, Place),
    /// `&xs[a nunchi b lopu]`; traps T10 on bad bounds.
    Slice {
        place: Place,
        lo: Operand,
        hi: Operand,
        inclusive: bool,
        mutable: bool,
        span: Span,
    },
    Binary(BinOp, OpTy, Operand, Operand, Span),
    Unary(UnOp, OpTy, Operand, Span),
    Aggregate(Agg, Vec<Operand>),
    /// The variant index of an enum value.
    Discriminant(Place),
    /// `a nunchi b lopu/varaku` of an integer type.
    Range(Operand, Operand, bool, IntTy),
}

#[derive(Clone, Debug)]
pub enum Callee {
    Fn(FnId, Vec<Ty>),
    /// A trait method; the types are `[idhi, trait args…, own generics…]`.
    Trait(FnId, Vec<Ty>),
    Value(Operand),
    Intrinsic(Intrinsic),
}

#[derive(Clone, Debug)]
pub enum Intrinsic {
    /// Turns an iterable into an iterator (§21.4). `copy`: items are values,
    /// not references.
    IntoIter { copy: bool, ty: Ty },
    /// `&maare iterator` → `undochu[item]`.
    IterNext,
    /// An f-string: text pieces around the displayed arguments (references).
    Format { texts: Vec<String>, tys: Vec<Ty> },
    /// Builds a `mukka` for a `migathavi` collector; `true` marks an argument
    /// whose elements are spliced in (`...xs`).
    Collect(Vec<bool>),
    /// The value of a module constant.
    Const(DefId),
    /// Something the interpreter can't run (`torch`, `python`).
    Unsupported(String),
}

pub enum Term {
    Goto(BlockId),
    Switch { discr: Operand, targets: Vec<(i128, BlockId)>, otherwise: BlockId },
    Call { callee: Callee, args: Vec<Operand>, dest: Place, next: BlockId, span: Span },
    Return,
    Unreachable,
}

impl Term {
    /// The blocks control can go to next.
    pub fn successors(&self) -> Vec<BlockId> {
        match self {
            Term::Goto(b) => vec![*b],
            Term::Switch { targets, otherwise, .. } => {
                targets.iter().map(|(_, b)| *b).chain(std::iter::once(*otherwise)).collect()
            }
            Term::Call { next, .. } => vec![*next],
            Term::Return | Term::Unreachable => Vec::new(),
        }
    }
}

impl Rvalue {
    pub fn operands(&self) -> Vec<&Operand> {
        match self {
            Rvalue::Use(o) | Rvalue::Unary(_, _, o, _) => vec![o],
            Rvalue::Binary(_, _, a, b, _) | Rvalue::Range(a, b, _, _) => vec![a, b],
            Rvalue::Slice { lo, hi, .. } => vec![lo, hi],
            Rvalue::Aggregate(_, ops) => ops.iter().collect(),
            Rvalue::Ref(..) | Rvalue::Discriminant(_) => Vec::new(),
        }
    }
}

impl Body {
    /// The locals that some operand moves out of (wholly or partly).
    pub fn moved_locals(&self) -> std::collections::HashSet<Local> {
        let mut out = std::collections::HashSet::new();
        let mut note = |o: &Operand| {
            if let Operand::Move(p) = o {
                out.insert(p.local);
            }
        };
        for b in &self.blocks {
            for s in &b.stmts {
                if let Stmt::Assign(_, rv) | Stmt::Replace(_, rv, _) = s {
                    rv.operands().into_iter().for_each(&mut note);
                }
            }
            match &b.term {
                Term::Switch { discr, .. } => note(discr),
                Term::Call { callee, args, .. } => {
                    if let Callee::Value(o) = callee {
                        note(o);
                    }
                    args.iter().for_each(&mut note);
                }
                _ => {}
            }
        }
        out
    }

    /// Which blocks can run, starting from the entry block.
    pub fn reachable(&self) -> Vec<bool> {
        let mut seen = vec![false; self.blocks.len()];
        let mut work = vec![0 as BlockId];
        while let Some(b) = work.pop() {
            let Some(slot) = seen.get_mut(b as usize) else { continue };
            if !std::mem::replace(slot, true) {
                work.extend(self.blocks[b as usize].term.successors());
            }
        }
        seen
    }
}

// ── place types ────────────────────────────────────────────────────────────

/// The field types of each variant of a type: one entry for a record, tuple
/// or range, two for `undochu`/`sare_thappu`, one per variant for an enum.
pub fn variant_fields(g: &crate::typeck::Globals, ty: &Ty) -> Vec<Vec<Ty>> {
    use crate::typeck::AdtKind;
    use crate::types::Con;
    match ty {
        Ty::Tuple(items) => vec![items.clone()],
        Ty::App(Con::Paridhi, a) => vec![vec![a[0].clone(), a[0].clone(), Ty::Bool]],
        Ty::App(Con::Undochu, a) => vec![vec![a[0].clone()], vec![]],
        Ty::App(Con::SareThappu, a) => vec![vec![a[0].clone()], vec![a[1].clone()]],
        Ty::App(Con::Adt(d), args) => match g.adts.get(d).map(|a| &a.kind) {
            Some(AdtKind::Enum(vs)) => {
                vs.iter().map(|v| v.fields.iter().map(|f| f.subst(args)).collect()).collect()
            }
            Some(AdtKind::Record(fs)) => vec![fs.iter().map(|f| f.ty.subst(args)).collect()],
            None => vec![Vec::new()],
        },
        _ => vec![Vec::new()],
    }
}

/// Whether values of the type are one of several variants.
pub fn is_enum(g: &crate::typeck::Globals, ty: &Ty) -> bool {
    use crate::types::Con;
    match ty {
        Ty::App(Con::Undochu | Con::SareThappu, _) => true,
        Ty::App(Con::Adt(d), _) => {
            matches!(g.adts.get(d).map(|a| &a.kind), Some(crate::typeck::AdtKind::Enum(_)))
        }
        _ => false,
    }
}

/// The type of the value at a place.
pub fn place_ty(g: &crate::typeck::Globals, body: &Body, p: &Place) -> Ty {
    use crate::types::Con;
    let mut ty = body.locals[p.local as usize].ty.clone();
    let mut variant = None;
    for proj in &p.proj {
        ty = match proj {
            Proj::Variant(v) => {
                variant = Some(*v as usize);
                continue;
            }
            Proj::Field(i) => {
                let vs = variant_fields(g, &ty);
                let fields = vs.get(variant.take().unwrap_or(0)).cloned().unwrap_or_default();
                fields.get(*i as usize).cloned().unwrap_or(Ty::Error)
            }
            Proj::Deref => match ty {
                Ty::Ref(_, inner) | Ty::Ptr(_, inner) => *inner,
                Ty::App(Con::Pette, a) => a[0].clone(),
                _ => Ty::Error,
            },
            Proj::Index(..) => match ty {
                Ty::Array(e, _) => *e,
                Ty::App(Con::Varusa | Con::Mukka, a) => a[0].clone(),
                _ => Ty::Error,
            },
        };
    }
    ty
}

/// Whether a type is reference-carrying (§9.3.1): a value of it may hold
/// borrows.
pub fn is_rc(ty: &Ty) -> bool {
    use crate::types::Con;
    match ty {
        Ty::Ref(..) | Ty::Fn(..) => true,
        Ty::App(Con::Mukka | Con::Pravaham, _) => true,
        Ty::App(Con::Undochu | Con::SareThappu, args) | Ty::Tuple(args) => args.iter().any(is_rc),
        _ => false,
    }
}

// ── printing (`antec emit mir`) ────────────────────────────────────────────

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = format!("_{}", self.local);
        for p in &self.proj {
            s = match p {
                Proj::Deref => format!("(*{s})"),
                Proj::Field(i) => format!("{s}.{i}"),
                Proj::Variant(v) => format!("({s} as #{v})"),
                Proj::Index(l, _) => format!("{s}[_{l}]"),
            };
        }
        write!(f, "{s}")
    }
}

impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operand::Copy(p) => write!(f, "{p}"),
            Operand::Move(p) => write!(f, "move {p}"),
            Operand::Const(c) => match c {
                Const::Unit => write!(f, "()"),
                Const::Bool(b) => write!(f, "{}", if *b { "avunu" } else { "kaadhu" }),
                Const::Int(i) => write!(f, "{i}"),
                Const::F32(x) => write!(f, "{x:?}f32"),
                Const::F64(x) => write!(f, "{x:?}"),
                Const::Char(c) => write!(f, "{c:?}"),
                Const::Str(s) => write!(f, "{s:?}"),
                Const::Fn(id, _) => write!(f, "fn#{id}"),
            },
        }
    }
}

fn list<T: fmt::Display>(items: &[T]) -> String {
    items.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ")
}

impl fmt::Display for Rvalue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rvalue::Use(o) => write!(f, "{o}"),
            Rvalue::Ref(m, p) => write!(f, "&{}{p}", if *m { "maare " } else { "" }),
            Rvalue::Slice { place, lo, hi, inclusive, .. } => {
                write!(f, "&{place}[{lo} nunchi {hi} {}]", if *inclusive { "varaku" } else { "lopu" })
            }
            Rvalue::Binary(op, _, a, b, _) => write!(f, "{a} {} {b}", op.as_str()),
            Rvalue::Unary(op, _, a, _) => write!(f, "{op:?}({a})"),
            Rvalue::Aggregate(agg, ops) => write!(f, "{agg:?}({})", list(ops)),
            Rvalue::Discriminant(p) => write!(f, "discriminant({p})"),
            Rvalue::Range(a, b, inc, _) => {
                write!(f, "{a} nunchi {b} {}", if *inc { "varaku" } else { "lopu" })
            }
        }
    }
}

impl Body {
    pub fn render(&self, fn_name: &dyn Fn(FnId) -> String, ty_name: &dyn Fn(&Ty) -> String) -> String {
        let mut out = format!("{} ({} params):\n", self.name, self.params);
        for (i, l) in self.locals.iter().enumerate() {
            let name = l.name.as_deref().map(|n| format!("  // {n}")).unwrap_or_default();
            out.push_str(&format!("    let _{i}: {};{name}\n", ty_name(&l.ty)));
        }
        for (i, b) in self.blocks.iter().enumerate() {
            out.push_str(&format!("  bb{i}:\n"));
            for s in &b.stmts {
                let line = match s {
                    Stmt::Assign(p, r) => format!("{p} = {r}"),
                    Stmt::Replace(p, r, _) => format!("replace {p} = {r}"),
                    Stmt::Drop(p, _) => format!("drop({p})"),
                    Stmt::StorageDead(l) => format!("dead(_{l})"),
                    Stmt::FreeBox(p, _) => format!("free_box({p})"),
                };
                out.push_str(&format!("    {line};\n"));
            }
            let term = match &b.term {
                Term::Goto(t) => format!("goto bb{t}"),
                Term::Switch { discr, targets, otherwise } => {
                    let arms: Vec<String> = targets.iter().map(|(v, t)| format!("{v} => bb{t}")).collect();
                    format!("switch {discr} [{}, _ => bb{otherwise}]", arms.join(", "))
                }
                Term::Call { callee, args, dest, next, .. } => {
                    let c = match callee {
                        Callee::Fn(id, _) => fn_name(*id),
                        Callee::Trait(id, _) => format!("<trait>{}", fn_name(*id)),
                        Callee::Value(o) => format!("({o})"),
                        Callee::Intrinsic(i) => format!("{i:?}"),
                    };
                    format!("{dest} = {c}({}) -> bb{next}", list(args))
                }
                Term::Return => "return".into(),
                Term::Unreachable => "unreachable".into(),
            };
            out.push_str(&format!("    {term};\n"));
        }
        out
    }
}
