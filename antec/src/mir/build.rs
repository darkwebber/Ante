//! Lowering type-checked "Ante" to MIR.
//!
//! Scopes are a stack of frames. A frame holds the locals to drop and the
//! `vellagane` blocks registered in it; leaving frames by any route (the end
//! of a block, `icchey`, `aapey`, `vadhiley`, `?`) runs the registered blocks
//! in LIFO order and then drops the locals in reverse order (§19.3–19.4).
//! Statement temporaries live in their own frame and are dropped at the end
//! of the statement, except in a binding, where they live as long as the
//! binding (§15.5).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::*;
use crate::ast::{
    self, Arm, BinOp, BindTarget, ClosureParam, Expr, ExprKind, FStrPart, FnSig, Ident, ItemKind, ParamKind,
    Pattern, PatternKind, Phrase, PhraseKind, StmtKind, UnOp,
};
use crate::resolve::SourceModule;
use crate::typeck::results::*;
use crate::typeck::solve::Solver;
use crate::typeck::{AdtKind, Generic, Globals, RecvMode, TraitRef, Typeck};
use crate::types::{Con, InferTable, Ty};

pub fn build_program<'a>(modules: &[SourceModule<'a>], tc: &'a Typeck<'a>) -> Program {
    let mut prog = Program::default();
    let drop_cache: Cache = Rc::new(RefCell::new(HashMap::new()));
    for m in modules {
        let Some(ast) = m.ast else { continue };
        let module = tc.globals.module_of_file(m.file);
        for item in &ast.items {
            match &item.kind {
                ItemKind::Fn(f) => build_fn(&mut prog, tc, &drop_cache, m.file, &f.sig, &f.body, None),
                ItemKind::Methods(ms) => {
                    for member in &ms.items {
                        if let ItemKind::Fn(f) = &member.kind {
                            build_fn(&mut prog, tc, &drop_cache, m.file, &f.sig, &f.body, None);
                        }
                    }
                }
                ItemKind::Impl(i) => {
                    for f in &i.items {
                        build_fn(&mut prog, tc, &drop_cache, m.file, &f.sig, &f.body, None);
                    }
                }
                ItemKind::Trait(t) => {
                    let def = tc.globals.res.modules[module].types.get(&t.name.name).copied();
                    for ti in &t.items {
                        if let Some(body) = &ti.default_body {
                            build_fn(&mut prog, tc, &drop_cache, m.file, &ti.sig, body, def);
                        }
                    }
                }
                ItemKind::Const { name, value, .. } => {
                    let Some(&def) = tc.globals.res.modules[module].values.get(&name.name) else { continue };
                    let ty = tc.globals.consts.get(&def).cloned().unwrap_or(Ty::Error);
                    let mut b = Builder::new(
                        tc,
                        drop_cache.clone(),
                        &mut prog,
                        m.file,
                        Vec::new(),
                        name.name.clone(),
                    );
                    b.body.kind = BodyKind::Const(def);
                    b.new_local(ty, None);
                    b.push_frame(false);
                    b.lower_into(value, Place::local(RETURN));
                    b.emit_cleanup(0);
                    b.terminate(Term::Return);
                    let body = b.into_body();
                    let id = prog.bodies.len() as BodyId;
                    prog.bodies.push(body);
                    prog.const_bodies.insert(def, id);
                }
                _ => {}
            }
        }
    }
    prog
}

#[allow(clippy::too_many_arguments)]
fn build_fn<'a>(
    prog: &mut Program,
    tc: &'a Typeck<'a>,
    cache: &Cache,
    file: FileId,
    sig_ast: &FnSig,
    block: &'a ast::Block,
    trait_def: Option<DefId>,
) {
    let g = &tc.globals;
    let Some(&fid) = g.fn_by_span.get(&(file, sig_ast.name.span.start)) else { return };
    let sig = &g.fns[fid];
    let mut generics = sig.generics.clone();
    if let Some(def) = trait_def {
        let n = g.traits.get(&def).map_or(1, |t| t.generics.len());
        generics[0].bounds.push(TraitRef { def, args: (1..n as u32).map(Ty::Param).collect() });
    }
    let mut b = Builder::new(tc, cache.clone(), prog, file, generics, sig.name.clone());
    b.body.kind = BodyKind::Fn(fid);
    b.new_local(sig.ret.clone(), None);
    b.push_frame(false);
    if let Some(r) = &sig.receiver {
        let ty = match r.mode {
            RecvMode::Value => r.ty.clone(),
            RecvMode::Ref => Ty::reference(false, r.ty.clone()),
            RecvMode::RefMut => Ty::reference(true, r.ty.clone()),
        };
        let l = b.new_local(ty.clone(), Some("naa".into()));
        b.naa = Some((l, false));
        b.register_drop(l, &ty);
    }
    for (p, f) in sig_ast.params.iter().filter(|p| p.kind != ParamKind::Receiver).zip(&sig.params) {
        let ty = match p.kind {
            ParamKind::Variadic => Ty::reference(false, Ty::App(Con::Mukka, vec![f.ty.clone()])),
            _ => f.ty.clone(),
        };
        let l = b.new_local(ty.clone(), Some(p.name.name.clone()));
        b.vars.insert(p.name.span.start, l);
        b.register_drop(l, &ty);
    }
    b.body.params = b.body.locals.len() as u32 - 1;
    b.lower_block(block);
    let ret = sig.ret.clone();
    let sare_unit = matches!(&ret, Ty::App(Con::SareThappu, a) if a[0] == Ty::Unit);
    if ret == Ty::Unit || sare_unit {
        b.finish_return(sare_unit);
    } else {
        b.terminate(Term::Unreachable);
    }
    let body = b.into_body();
    let id = prog.bodies.len() as BodyId;
    prog.bodies.push(body);
    prog.fn_bodies.insert(fid, id);
}

type Cache = Rc<RefCell<HashMap<Ty, bool>>>;

struct Frame<'a> {
    drops: Vec<(Local, Ty)>,
    defers: Vec<(Local, &'a ast::Block)>,
    temps: bool,
}

struct LoopCx {
    brk: BlockId,
    cont: BlockId,
    depth: usize,
}

/// A phrase operand evaluated ahead of binding.
#[derive(Clone)]
enum Val {
    Place(Place, Ty),
    Op(Operand),
}

struct Builder<'a, 'p> {
    tc: &'a Typeck<'a>,
    g: &'a Globals<'a>,
    cache: Cache,
    prog: &'p mut Program,
    file: FileId,
    generics: Vec<Generic>,
    body: Body,
    cur: BlockId,
    vars: HashMap<usize, Local>,
    /// For a closure: captured locals by declaration, and whether by reference.
    captures: HashMap<usize, (Local, bool)>,
    naa: Option<(Local, bool)>,
    frames: Vec<Frame<'a>>,
    loops: Vec<LoopCx>,
    /// Flags of `vellagane` blocks, cleared on entry.
    defer_flags: Vec<Local>,
}

impl<'a, 'p> Builder<'a, 'p> {
    fn new(
        tc: &'a Typeck<'a>,
        cache: Cache,
        prog: &'p mut Program,
        file: FileId,
        generics: Vec<Generic>,
        name: String,
    ) -> Self {
        Builder {
            tc,
            g: &tc.globals,
            cache,
            prog,
            file,
            generics,
            body: Body {
                name,
                kind: BodyKind::Closure,
                file: Some(file),
                locals: Vec::new(),
                params: 0,
                blocks: vec![Block { stmts: Vec::new(), term: Term::Unreachable }],
            },
            cur: 0,
            vars: HashMap::new(),
            captures: HashMap::new(),
            naa: None,
            frames: Vec::new(),
            loops: Vec::new(),
            defer_flags: Vec::new(),
        }
    }

    fn into_body(mut self) -> Body {
        let inits: Vec<Stmt> = self
            .defer_flags
            .iter()
            .map(|&f| Stmt::Assign(Place::local(f), Rvalue::Use(Operand::Const(Const::Bool(false)))))
            .collect();
        self.body.blocks[0].stmts.splice(0..0, inits);
        self.body
    }

    // ── basics ─────────────────────────────────────────────────────────────

    fn new_local(&mut self, ty: Ty, name: Option<String>) -> Local {
        self.body.locals.push(LocalDecl { ty, name });
        (self.body.locals.len() - 1) as Local
    }

    fn new_block(&mut self) -> BlockId {
        self.body.blocks.push(Block { stmts: Vec::new(), term: Term::Unreachable });
        (self.body.blocks.len() - 1) as BlockId
    }

    fn emit(&mut self, s: Stmt) {
        self.body.blocks[self.cur as usize].stmts.push(s);
    }

    fn assign(&mut self, p: Place, rv: Rvalue) {
        self.emit(Stmt::Assign(p, rv));
    }

    /// Ends the current block; code after this goes to a fresh, unreachable block.
    fn terminate(&mut self, t: Term) {
        self.body.blocks[self.cur as usize].term = t;
        self.cur = self.new_block();
    }

    fn goto(&mut self, target: BlockId) {
        self.body.blocks[self.cur as usize].term = Term::Goto(target);
        self.cur = target;
    }

    fn jump(&mut self, target: BlockId) {
        self.body.blocks[self.cur as usize].term = Term::Goto(target);
    }

    fn ty_of(&self, e: &Expr) -> Ty {
        self.tc.results.expr_ty.get(&expr_id(self.file, e.span)).cloned().unwrap_or(Ty::Error)
    }

    fn is_copy(&self, ty: &Ty) -> bool {
        Solver { g: self.g, generics: &self.generics }.is_copy(&mut InferTable::default(), ty)
    }

    fn needs_drop(&self, ty: &Ty) -> bool {
        needs_drop(self.g, &self.generics, &self.cache, ty, &mut HashSet::new())
    }

    fn register_drop(&mut self, l: Local, ty: &Ty) {
        if self.needs_drop(ty) {
            let frame = self.frames.iter_mut().rev().find(|f| !f.temps).expect("a scope frame");
            frame.drops.push((l, ty.clone()));
        }
    }

    fn temp(&mut self, ty: Ty) -> Local {
        let l = self.new_local(ty.clone(), None);
        if self.needs_drop(&ty) {
            if let Some(f) = self.frames.last_mut() {
                f.drops.push((l, ty));
            }
        }
        l
    }

    fn by_type(&self, p: Place, ty: &Ty) -> Operand {
        if self.is_copy(ty) {
            Operand::Copy(p)
        } else {
            Operand::Move(p)
        }
    }

    // ── frames and exits ───────────────────────────────────────────────────

    fn push_frame(&mut self, temps: bool) {
        self.frames.push(Frame { drops: Vec::new(), defers: Vec::new(), temps });
    }

    /// Emits the cleanup of frames `depth..` (innermost first) without popping them.
    fn emit_cleanup(&mut self, depth: usize) {
        for i in (depth..self.frames.len()).rev() {
            let defers: Vec<(Local, &'a ast::Block)> = self.frames[i].defers.iter().rev().copied().collect();
            for (flag, block) in defers {
                let run = self.new_block();
                let after = self.new_block();
                self.body.blocks[self.cur as usize].term = Term::Switch {
                    discr: Operand::Copy(Place::local(flag)),
                    targets: vec![(1, run)],
                    otherwise: after,
                };
                self.cur = run;
                self.assign(Place::local(flag), Rvalue::Use(Operand::Const(Const::Bool(false))));
                self.lower_block(block);
                self.jump(after);
                self.cur = after;
            }
            let drops: Vec<(Local, Ty)> = self.frames[i].drops.iter().rev().cloned().collect();
            for (l, ty) in drops {
                self.emit(Stmt::Drop(Place::local(l), ty));
            }
        }
    }

    fn pop_frame(&mut self) {
        let depth = self.frames.len() - 1;
        self.emit_cleanup(depth);
        self.frames.pop();
    }

    fn finish_return(&mut self, sare_unit: bool) {
        let value = if sare_unit {
            Rvalue::Aggregate(Agg::Adt(AdtRef::SareThappu, 0), vec![Operand::Const(Const::Unit)])
        } else {
            Rvalue::Use(Operand::Const(Const::Unit))
        };
        self.assign(Place::local(RETURN), value);
        self.emit_cleanup(0);
        self.terminate(Term::Return);
    }

    // ── statements ─────────────────────────────────────────────────────────

    fn lower_block(&mut self, block: &'a ast::Block) {
        self.push_frame(false);
        for stmt in &block.stmts {
            self.lower_stmt(stmt);
        }
        self.pop_frame();
    }

    fn lower_stmt(&mut self, stmt: &'a ast::Stmt) {
        self.push_frame(true);
        let mut extend = false;
        match &stmt.kind {
            StmtKind::Binding { target, value, .. } => {
                extend = true;
                match target {
                    BindTarget::Name(n) => {
                        let ty = self.local_ty(n.span.start);
                        let l = self.new_local(ty.clone(), Some(n.name.clone()));
                        self.lower_into(value, Place::local(l));
                        self.vars.insert(n.span.start, l);
                        self.register_drop(l, &ty);
                    }
                    BindTarget::Tuple(names) => {
                        let tt = self.ty_of(value);
                        let t = self.temp(tt);
                        self.lower_into(value, Place::local(t));
                        for (i, n) in names.iter().enumerate() {
                            let ty = self.local_ty(n.span.start);
                            let l = self.new_local(ty.clone(), Some(n.name.clone()));
                            let op = self.by_type(Place::local(t).field(i as u32), &ty);
                            self.assign(Place::local(l), Rvalue::Use(op));
                            self.vars.insert(n.span.start, l);
                            self.register_drop(l, &ty);
                        }
                    }
                }
            }
            StmtKind::Assign { place, value } => {
                let pt = self.ty_of(place);
                let p = self.place_of(place);
                let v = self.lower_operand(value);
                if self.needs_drop(&pt) {
                    self.emit(Stmt::Replace(p, Rvalue::Use(v), pt));
                } else {
                    self.assign(p, Rvalue::Use(v));
                }
            }
            StmtKind::Compound { place, op, value } => self.lower_compound(place, *op, value),
            StmtKind::Return(value) => {
                match value {
                    Some(e) => self.lower_into(e, Place::local(RETURN)),
                    None => self.assign(Place::local(RETURN), Rvalue::Use(Operand::Const(Const::Unit))),
                }
                self.emit_cleanup(0);
                self.terminate(Term::Return);
            }
            StmtKind::If { branches, else_block } => {
                let join = self.new_block();
                for (cond, body) in branches {
                    let c = self.lower_operand(cond);
                    let then = self.new_block();
                    let next = self.new_block();
                    self.body.blocks[self.cur as usize].term =
                        Term::Switch { discr: c, targets: vec![(1, then)], otherwise: next };
                    self.cur = then;
                    self.lower_block(body);
                    self.jump(join);
                    self.cur = next;
                }
                if let Some(body) = else_block {
                    self.lower_block(body);
                }
                self.goto(join);
            }
            StmtKind::While { cond, body } => {
                let head = self.new_block();
                let exit = self.new_block();
                self.goto(head);
                let c = self.lower_operand(cond);
                let inner = self.new_block();
                self.body.blocks[self.cur as usize].term =
                    Term::Switch { discr: c, targets: vec![(1, inner)], otherwise: exit };
                self.cur = inner;
                self.loops.push(LoopCx { brk: exit, cont: head, depth: self.frames.len() });
                self.lower_block(body);
                self.loops.pop();
                self.jump(head);
                self.cur = exit;
            }
            StmtKind::For { iter, pattern, body } => self.lower_for(iter, pattern, body),
            StmtKind::Match { scrutinee, arms } => self.lower_match(scrutinee, arms),
            StmtKind::Break | StmtKind::Continue => {
                if let Some(lp) = self.loops.last() {
                    let (target, depth) = if matches!(stmt.kind, StmtKind::Break) {
                        (lp.brk, lp.depth)
                    } else {
                        (lp.cont, lp.depth)
                    };
                    self.emit_cleanup(depth);
                    self.terminate(Term::Goto(target));
                }
            }
            StmtKind::Defer(block) => {
                let flag = self.new_local(Ty::Bool, Some("vellagane".into()));
                self.defer_flags.push(flag);
                self.assign(Place::local(flag), Rvalue::Use(Operand::Const(Const::Bool(true))));
                let frame = self.frames.iter_mut().rev().find(|f| !f.temps).expect("a scope frame");
                frame.defers.push((flag, block));
            }
            StmtKind::Unsafe(block) => self.lower_block(block),
            StmtKind::Expr(e) => {
                let ty = self.ty_of(e);
                let t = self.temp(ty);
                self.lower_into(e, Place::local(t));
            }
        }
        if extend {
            // Temporaries of a binding live as long as the binding (§15.5).
            let temps = self.frames.pop().expect("temp frame");
            let scope = self.frames.iter_mut().rev().find(|f| !f.temps).expect("a scope frame");
            scope.drops.extend(temps.drops);
        } else {
            self.pop_frame();
        }
    }

    fn local_ty(&self, decl: usize) -> Ty {
        self.tc.results.local_ty.get(&(self.file, decl)).map(|(t, _)| t.clone()).unwrap_or(Ty::Error)
    }

    fn lower_compound(&mut self, place: &'a Expr, op: BinOp, value: &'a Expr) {
        let mut pt = self.ty_of(place);
        let mut p = self.place_of(place);
        if let Ty::Ref(_, inner) = &pt {
            if op_ty(inner).is_some() {
                p = p.deref();
                pt = (**inner).clone();
            }
        }
        let v = self.scalar_operand(value);
        match op_ty(&pt) {
            Some(t) => {
                let span = place.span;
                self.assign(p.clone(), Rvalue::Binary(op, t, Operand::Copy(p), v, span));
            }
            None => {
                let vt = self.ty_of(value);
                let vp = self.operand_place(v, vt);
                let result = self.trait_binary(op, &pt, p.clone(), vp, place.span);
                let rt = self.temp(pt.clone());
                self.assign(Place::local(rt), result);
                self.emit(Stmt::Replace(p, Rvalue::Use(Operand::Move(Place::local(rt))), pt));
            }
        }
    }

    // ── loops and matches ──────────────────────────────────────────────────

    fn iterable_operand(&mut self, e: &'a Expr) -> Operand {
        let ty = self.ty_of(e);
        if is_place(e) && owned_collection(&ty) {
            let p = self.place_of(e);
            let t = self.temp(Ty::reference(false, ty));
            self.assign(Place::local(t), Rvalue::Ref(false, p));
            return Operand::Copy(Place::local(t));
        }
        self.lower_operand(e)
    }

    fn make_iter(&mut self, op: Operand, ty: Ty, item: &Ty, span: Span) -> Local {
        let it_ty = Ty::App(Con::Pravaham, vec![item.clone()]);
        let it = self.temp(it_ty);
        let copy = !matches!(item, Ty::Ref(..));
        self.call(Callee::Intrinsic(Intrinsic::IntoIter { copy, ty }), vec![op], Place::local(it), span);
        it
    }

    fn lower_for(&mut self, iter: &'a Expr, pattern: &'a Pattern, body: &'a ast::Block) {
        let item =
            self.tc.results.for_items.get(&expr_id(self.file, iter.span)).cloned().unwrap_or(Ty::Error);
        let op = self.iterable_operand(iter);
        let ity = self.ty_of(iter);
        let it = self.make_iter(op, ity, &item, iter.span);
        let head = self.new_block();
        let exit = self.new_block();
        self.goto(head);
        let next_ty = Ty::App(Con::Undochu, vec![item.clone()]);
        let next = self.new_local(next_ty, None);
        let it_ref = self.new_local(Ty::Error, None);
        self.assign(Place::local(it_ref), Rvalue::Ref(true, Place::local(it)));
        self.call(
            Callee::Intrinsic(Intrinsic::IterNext),
            vec![Operand::Copy(Place::local(it_ref))],
            Place::local(next),
            iter.span,
        );
        let d = self.new_local(Ty::ANKAM, None);
        self.assign(Place::local(d), Rvalue::Discriminant(Place::local(next)));
        let inner = self.new_block();
        self.body.blocks[self.cur as usize].term = Term::Switch {
            discr: Operand::Copy(Place::local(d)),
            targets: vec![(0, inner)],
            otherwise: exit,
        };
        self.cur = inner;
        self.loops.push(LoopCx { brk: exit, cont: head, depth: self.frames.len() });
        self.push_frame(false);
        self.bind_pat(&Place::local(next).field(0), pattern, &HashMap::new());
        self.lower_block(body);
        self.pop_frame();
        self.loops.pop();
        self.jump(head);
        self.cur = exit;
    }

    fn lower_match(&mut self, scrutinee: &'a Expr, arms: &'a [Arm]) {
        let place = if is_place(scrutinee) {
            self.place_of(scrutinee)
        } else {
            let ty = self.ty_of(scrutinee);
            let t = self.temp(ty);
            self.lower_into(scrutinee, Place::local(t));
            Place::local(t)
        };
        let join = self.new_block();
        for arm in arms {
            let next = self.new_block();
            self.test_pat(&place, &arm.pattern, next);
            self.push_frame(false);
            self.bind_pat(&place, &arm.pattern, &HashMap::new());
            self.lower_block(&arm.body);
            self.pop_frame();
            self.jump(join);
            self.cur = next;
        }
        self.terminate(Term::Unreachable);
        self.cur = join;
    }

    // ── patterns (§14) ─────────────────────────────────────────────────────

    /// Peels references the pattern matches through (§14.2).
    fn pat_place(&self, place: &Place, p: &Pattern) -> (Place, Ty) {
        let mut ty = self.tc.results.pat_ty.get(&expr_id(self.file, p.span)).cloned().unwrap_or(Ty::Error);
        let mut place = place.clone();
        if !matches!(p.kind, PatternKind::Bind { .. } | PatternKind::Wild) {
            while let Ty::Ref(_, inner) = ty {
                place = place.deref();
                ty = *inner;
            }
        }
        (place, ty)
    }

    fn branch_if(&mut self, cond: Operand, fail: BlockId) {
        let ok = self.new_block();
        self.body.blocks[self.cur as usize].term =
            Term::Switch { discr: cond, targets: vec![(1, ok)], otherwise: fail };
        self.cur = ok;
    }

    fn compare(&mut self, op: BinOp, t: OpTy, a: Operand, b: Operand, span: Span) -> Operand {
        let l = self.new_local(Ty::Bool, None);
        self.assign(Place::local(l), Rvalue::Binary(op, t, a, b, span));
        Operand::Copy(Place::local(l))
    }

    /// Emits the tests of `p` against `place`, jumping to `fail` on a mismatch.
    fn test_pat(&mut self, place: &Place, p: &'a Pattern, fail: BlockId) {
        let (place, ty) = self.pat_place(place, p);
        match &p.kind {
            PatternKind::Wild | PatternKind::Bind { .. } => {}
            PatternKind::Literal(e) => {
                let Some(t) = op_ty(&ty) else { return };
                let c = literal_const(e, &ty);
                let ok = self.compare(BinOp::Eq, t, Operand::Copy(place), Operand::Const(c), p.span);
                self.branch_if(ok, fail);
            }
            PatternKind::Range { lo, hi, inclusive } => {
                let Some(t) = op_ty(&ty) else { return };
                let a = literal_const(lo, &ty);
                let b = literal_const(hi, &ty);
                let ok = self.compare(BinOp::Ge, t, Operand::Copy(place.clone()), Operand::Const(a), p.span);
                self.branch_if(ok, fail);
                let op = if *inclusive { BinOp::Le } else { BinOp::Lt };
                let ok = self.compare(op, t, Operand::Copy(place), Operand::Const(b), p.span);
                self.branch_if(ok, fail);
            }
            PatternKind::Ctor { args, .. } => {
                let Some(&(_, variant)) = self.tc.results.pat_ctors.get(&expr_id(self.file, p.span)) else {
                    return;
                };
                let d = self.new_local(Ty::ANKAM, None);
                self.assign(Place::local(d), Rvalue::Discriminant(place.clone()));
                let ok = self.new_block();
                self.body.blocks[self.cur as usize].term = Term::Switch {
                    discr: Operand::Copy(Place::local(d)),
                    targets: vec![(variant as i128, ok)],
                    otherwise: fail,
                };
                self.cur = ok;
                for (i, sub) in args.iter().flatten().enumerate() {
                    self.test_pat(&place.field(i as u32), sub, fail);
                }
            }
            PatternKind::Record { fields, .. } => {
                for (name, sub) in fields {
                    if let (Some(sub), Some(i)) = (sub, self.field_index(&ty, &name.name)) {
                        self.test_pat(&place.field(i), sub, fail);
                    }
                }
            }
            PatternKind::Tuple(items) => {
                for (i, sub) in items.iter().enumerate() {
                    self.test_pat(&place.field(i as u32), sub, fail);
                }
            }
            PatternKind::Or(alts) => {
                let ok = self.new_block();
                for (i, alt) in alts.iter().enumerate() {
                    if i + 1 == alts.len() {
                        self.test_pat(&place, alt, fail);
                    } else {
                        let next = self.new_block();
                        self.test_pat(&place, alt, next);
                        self.jump(ok);
                        self.cur = next;
                    }
                }
                self.goto(ok);
            }
        }
    }

    fn field_index(&self, ty: &Ty, name: &str) -> Option<u32> {
        let Ty::App(Con::Adt(d), _) = ty else { return None };
        match &self.g.adts.get(d)?.kind {
            AdtKind::Record(fs) => fs.iter().position(|f| f.name == name).map(|i| i as u32),
            AdtKind::Enum(_) => None,
        }
    }

    /// Binds the names of `p` (whose tests passed). `shared` maps names of
    /// an OR-pattern's alternatives to the locals of its first alternative.
    fn bind_pat(&mut self, place: &Place, p: &'a Pattern, shared: &HashMap<String, Local>) {
        let (place, ty) = self.pat_place(place, p);
        match &p.kind {
            PatternKind::Wild | PatternKind::Literal(_) | PatternKind::Range { .. } => {}
            PatternKind::Bind { name, .. } => self.bind_name(name, place, shared),
            PatternKind::Ctor { args, .. } => {
                for (i, sub) in args.iter().flatten().enumerate() {
                    self.bind_pat(&place.field(i as u32), sub, shared);
                }
            }
            PatternKind::Record { fields, .. } => {
                for (name, sub) in fields {
                    let Some(i) = self.field_index(&ty, &name.name) else { continue };
                    match sub {
                        Some(sub) => self.bind_pat(&place.field(i), sub, shared),
                        None => self.bind_name(name, place.field(i), shared),
                    }
                }
            }
            PatternKind::Tuple(items) => {
                for (i, sub) in items.iter().enumerate() {
                    self.bind_pat(&place.field(i as u32), sub, shared);
                }
            }
            PatternKind::Or(alts) => {
                // Locals come from the first alternative, which the body's names resolve to.
                let mut names = Vec::new();
                pattern_names(&alts[0], &mut names);
                let mut map = shared.clone();
                for n in &names {
                    if !map.contains_key(&n.name) {
                        let ty = self.local_ty(n.span.start);
                        let l = self.new_local(ty.clone(), Some(n.name.clone()));
                        self.vars.insert(n.span.start, l);
                        self.register_drop(l, &ty);
                        map.insert(n.name.clone(), l);
                    }
                }
                let join = self.new_block();
                for (i, alt) in alts.iter().enumerate() {
                    if i + 1 < alts.len() {
                        let next = self.new_block();
                        self.test_pat(&place, alt, next);
                        self.bind_pat(&place, alt, &map);
                        self.jump(join);
                        self.cur = next;
                    } else {
                        self.bind_pat(&place, alt, &map);
                        self.jump(join);
                    }
                }
                self.cur = join;
            }
        }
    }

    fn bind_name(&mut self, name: &Ident, place: Place, shared: &HashMap<String, Local>) {
        let mode =
            self.tc.results.binds.get(&(self.file, name.span.start)).copied().unwrap_or(BindMode::Move);
        let local = match shared.get(&name.name) {
            Some(&l) => l,
            None => {
                let ty = self.local_ty(name.span.start);
                let l = self.new_local(ty.clone(), Some(name.name.clone()));
                self.vars.insert(name.span.start, l);
                self.register_drop(l, &ty);
                l
            }
        };
        let rv = match mode {
            BindMode::Copy => Rvalue::Use(Operand::Copy(place)),
            BindMode::Move => Rvalue::Use(Operand::Move(place)),
            BindMode::Ref(m) => Rvalue::Ref(m, place),
        };
        self.assign(Place::local(local), rv);
    }

    // ── places and operands ────────────────────────────────────────────────

    fn var_place(&self, decl: usize) -> Place {
        if let Some(&l) = self.vars.get(&decl) {
            return Place::local(l);
        }
        match self.captures.get(&decl) {
            Some(&(l, true)) => Place::local(l).deref(),
            Some(&(l, false)) => Place::local(l),
            None => Place::local(RETURN),
        }
    }

    fn naa_place(&self) -> Place {
        match self.naa {
            Some((l, true)) => Place::local(l).deref(),
            Some((l, false)) => Place::local(l),
            None => Place::local(RETURN),
        }
    }

    fn apply_steps(mut p: Place, steps: &[FieldStep]) -> Place {
        for s in steps {
            for _ in 0..s.derefs {
                p = p.deref();
            }
            p = p.field(s.index);
        }
        p
    }

    /// The place a place expression denotes, if it is one.
    fn try_place(&mut self, e: &'a Expr) -> Option<Place> {
        let id = expr_id(self.file, e.span);
        match &e.kind {
            ExprKind::Path(_) => match self.tc.results.paths.get(&id)? {
                PathRes::Local { decl, steps } => Some(Self::apply_steps(self.var_place(*decl), steps)),
                PathRes::Naa { steps } => Some(Self::apply_steps(self.naa_place(), steps)),
                PathRes::Const { def, steps } => {
                    let (def, steps) = (*def, steps.clone());
                    let ty = self.g.consts.get(&def).cloned().unwrap_or(Ty::Error);
                    let t = self.temp(ty);
                    self.call(Callee::Intrinsic(Intrinsic::Const(def)), Vec::new(), Place::local(t), e.span);
                    Some(Self::apply_steps(Place::local(t), &steps))
                }
                _ => None,
            },
            ExprKind::Field { base, .. } | ExprKind::TupleIndex { base, .. } => {
                let step = self.tc.results.fields.get(&id)?.clone();
                let b = self.place_of(base);
                Some(Self::apply_steps(b, &[step]))
            }
            ExprKind::Index { base, indices } if indices.len() == 1 => {
                if matches!(indices[0].kind, ExprKind::Range { .. }) {
                    return None;
                }
                let derefs = self.tc.results.index_derefs.get(&id).copied().unwrap_or(0);
                let mut b = self.place_of(base);
                for _ in 0..derefs {
                    b = b.deref();
                }
                let i = self.lower_operand(&indices[0]);
                let it = self.ty_of(&indices[0]);
                let il = self.new_local(it, None);
                self.assign(Place::local(il), Rvalue::Use(i));
                Some(b.project(Proj::Index(il, e.span)))
            }
            ExprKind::Unary { op: UnOp::Deref, operand } => Some(self.place_of(operand).deref()),
            _ => None,
        }
    }

    /// The place of `e`, evaluating it into a temporary when it isn'a one.
    fn place_of(&mut self, e: &'a Expr) -> Place {
        if let Some(p) = self.try_place(e) {
            return p;
        }
        let ty = self.ty_of(e);
        let t = self.temp(ty);
        self.lower_into(e, Place::local(t));
        Place::local(t)
    }

    fn lower_operand(&mut self, e: &'a Expr) -> Operand {
        match &e.kind {
            ExprKind::Int(lit) => {
                return Operand::Const(Const::Int(i128::from_str_radix(&lit.digits, lit.radix).unwrap_or(0)))
            }
            ExprKind::Bool(b) => return Operand::Const(Const::Bool(*b)),
            ExprKind::Str(s) => return Operand::Const(Const::Str(Rc::from(s.as_str()))),
            ExprKind::Char(c) => return Operand::Const(Const::Char(*c)),
            ExprKind::Unit => return Operand::Const(Const::Unit),
            _ => {}
        }
        if is_place(e) {
            if let Some(p) = self.try_place(e) {
                let ty = self.ty_of(e);
                return self.by_type(p, &ty);
            }
        }
        let ty = self.ty_of(e);
        let t = self.temp(ty);
        self.lower_into(e, Place::local(t));
        Operand::Move(Place::local(t))
    }

    /// An operand of a built-in operator: `&ankam` reads through the reference.
    fn scalar_operand(&mut self, e: &'a Expr) -> Operand {
        let ty = self.ty_of(e);
        if let Ty::Ref(_, inner) = &ty {
            if op_ty(inner).is_some() {
                let p = self.place_of(e);
                return Operand::Copy(p.deref());
            }
        }
        self.lower_operand(e)
    }

    fn operand_place(&mut self, op: Operand, ty: Ty) -> Place {
        match op {
            Operand::Copy(p) | Operand::Move(p) => p,
            c @ Operand::Const(_) => {
                let t = self.temp(ty);
                self.assign(Place::local(t), Rvalue::Use(c));
                Place::local(t)
            }
        }
    }

    fn ref_to(&mut self, p: Place, ty: &Ty, mutable: bool) -> Operand {
        let t = self.new_local(Ty::reference(mutable, ty.clone()), None);
        self.assign(Place::local(t), Rvalue::Ref(mutable, p));
        Operand::Copy(Place::local(t))
    }

    fn call(&mut self, callee: Callee, args: Vec<Operand>, dest: Place, span: Span) {
        let next = self.new_block();
        self.body.blocks[self.cur as usize].term = Term::Call { callee, args, dest, next, span };
        self.cur = next;
    }

    // ── expressions ────────────────────────────────────────────────────────

    fn lower_into(&mut self, e: &'a Expr, dest: Place) {
        let ty = self.ty_of(e);
        let id = expr_id(self.file, e.span);
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Str(_) | ExprKind::Char(_) | ExprKind::Unit => {
                let op = self.lower_operand(e);
                self.assign(dest, Rvalue::Use(op));
            }
            ExprKind::Float(text) => {
                let c = float_const(text, &ty);
                self.assign(dest, Rvalue::Use(Operand::Const(c)));
            }
            ExprKind::FStr(parts) => {
                let mut texts = vec![String::new()];
                let mut tys = Vec::new();
                let mut args = Vec::new();
                for part in parts {
                    match part {
                        FStrPart::Text(t) => texts.last_mut().expect("non-empty").push_str(t),
                        FStrPart::Expr(x) => {
                            let xt = self.ty_of(x);
                            let p = self.place_of(x);
                            args.push(self.ref_to(p, &xt, false));
                            tys.push(xt);
                            texts.push(String::new());
                        }
                    }
                }
                self.call(Callee::Intrinsic(Intrinsic::Format { texts, tys }), args, dest, e.span);
            }
            ExprKind::Ledhu => self.assign(dest, Rvalue::Aggregate(Agg::Adt(AdtRef::Undochu, 1), Vec::new())),
            ExprKind::BareSare => self.assign(
                dest,
                Rvalue::Aggregate(Agg::Adt(AdtRef::SareThappu, 0), vec![Operand::Const(Const::Unit)]),
            ),
            ExprKind::Hole => self.unsupported("`_`", dest, e.span),
            ExprKind::Path(_) => match self.tc.results.paths.get(&id).cloned() {
                Some(PathRes::Fn { fid, tys }) => {
                    self.assign(dest, Rvalue::Use(Operand::Const(Const::Fn(fid, tys))))
                }
                Some(PathRes::Variant { adt, variant }) => {
                    self.assign(dest, Rvalue::Aggregate(Agg::Adt(AdtRef::Adt(adt), variant), Vec::new()))
                }
                Some(PathRes::Opaque) | None => self.unsupported("ee peru", dest, e.span),
                Some(_) => {
                    let op = self.lower_operand(e);
                    self.assign(dest, Rvalue::Use(op));
                }
            },
            ExprKind::Call { callee, phrases, .. } => self.lower_call(e, callee, phrases, None, dest),
            ExprKind::Try(inner) => {
                let it = self.ty_of(inner);
                let t = self.temp(it.clone());
                self.lower_into(inner, Place::local(t));
                self.lower_try(Place::local(t), &it, dest, e.span);
            }
            ExprKind::Pipe { input, stage } => {
                let it = self.ty_of(input);
                let inj = if is_place(input) {
                    let p = self.place_of(input);
                    Val::Place(p, it)
                } else {
                    let t = self.temp(it.clone());
                    self.lower_into(input, Place::local(t));
                    Val::Place(Place::local(t), it)
                };
                match &stage.kind {
                    ExprKind::Call { callee, phrases, .. } => {
                        self.lower_call(stage, callee, phrases, Some(inj), dest)
                    }
                    ExprKind::Try(inner) => {
                        if let ExprKind::Call { callee, phrases, .. } = &inner.kind {
                            let ct = self.ty_of(inner);
                            let t = self.temp(ct.clone());
                            self.lower_call(inner, callee, phrases, Some(inj), Place::local(t));
                            self.lower_try(Place::local(t), &ct, dest, stage.span);
                        }
                    }
                    _ => self.unsupported("pipe", dest, e.span),
                }
            }
            ExprKind::Binary { op, lhs, rhs } => self.lower_binary(*op, lhs, rhs, dest, e.span),
            ExprKind::Unary { op, operand } => self.lower_unary(*op, operand, &ty, dest, e.span),
            ExprKind::Field { .. } | ExprKind::TupleIndex { .. } | ExprKind::Index { .. } => {
                match self.try_place(e) {
                    Some(p) => {
                        let op = self.by_type(p, &ty);
                        self.assign(dest, Rvalue::Use(op));
                    }
                    None => self.unsupported("ee index", dest, e.span),
                }
            }
            ExprKind::Range { lo, hi, inclusive } => {
                let a = self.scalar_operand(lo);
                let b = self.scalar_operand(hi);
                let it = match &ty {
                    Ty::App(Con::Paridhi, args) => match &args[0] {
                        Ty::Int(i) => *i,
                        _ => crate::types::IntTy::ANKAM,
                    },
                    _ => crate::types::IntTy::ANKAM,
                };
                self.assign(dest, Rvalue::Range(a, b, *inclusive, it));
            }
            ExprKind::Closure { moves, params, body } => self.lower_closure(e, *moves, params, body, dest),
            ExprKind::Ctor { args, .. } => {
                let Some(&(adt, variant)) = self.tc.results.ctors.get(&id) else {
                    return self.unsupported("constructor", dest, e.span);
                };
                let ops = args.iter().map(|a| self.lower_operand(a)).collect();
                self.assign(dest, Rvalue::Aggregate(Agg::Adt(adt, variant), ops));
            }
            ExprKind::Record { fields, .. } => {
                let Some(&def) = self.tc.results.records.get(&id) else {
                    return self.unsupported("record", dest, e.span);
                };
                let names: Vec<String> = match self.g.adts.get(&def).map(|a| &a.kind) {
                    Some(AdtKind::Record(fs)) => fs.iter().map(|f| f.name.clone()).collect(),
                    _ => Vec::new(),
                };
                // Fields are evaluated in source order (§17.2), stored in declaration order.
                let mut vals: HashMap<String, Operand> = HashMap::new();
                for (n, v) in fields {
                    let op = self.lower_operand(v);
                    let vt = self.ty_of(v);
                    let op = self.stable(op, vt);
                    vals.insert(n.name.clone(), op);
                }
                let ops =
                    names.iter().map(|n| vals.remove(n).unwrap_or(Operand::Const(Const::Unit))).collect();
                self.assign(dest, Rvalue::Aggregate(Agg::Adt(AdtRef::Adt(def), 0), ops));
            }
            ExprKind::Tuple(items) => {
                let ops = self.operands(items);
                self.assign(dest, Rvalue::Aggregate(Agg::Tuple, ops));
            }
            ExprKind::List(items) => {
                let agg = if matches!(ty, Ty::Array(..)) { Agg::Array } else { Agg::Varusa };
                let ops = self.operands(items);
                self.assign(dest, Rvalue::Aggregate(agg, ops));
            }
            ExprKind::Map(pairs) => {
                let mut ops = Vec::new();
                for (k, v) in pairs {
                    let ko = self.lower_operand(k);
                    let kt = self.ty_of(k);
                    ops.push(self.stable(ko, kt));
                    let vo = self.lower_operand(v);
                    let vt = self.ty_of(v);
                    ops.push(self.stable(vo, vt));
                }
                self.assign(dest, Rvalue::Aggregate(Agg::Jodi, ops));
            }
        }
    }

    /// Operands evaluated left to right; each is fixed in a temporary first
    /// so later operands can'a change it.
    fn operands(&mut self, items: &'a [Expr]) -> Vec<Operand> {
        let mut out = Vec::new();
        for x in items {
            let op = self.lower_operand(x);
            let t = self.ty_of(x);
            out.push(self.stable(op, t));
        }
        out
    }

    fn stable(&mut self, op: Operand, ty: Ty) -> Operand {
        match op {
            Operand::Copy(p) => {
                let t = self.new_local(ty, None);
                self.assign(Place::local(t), Rvalue::Use(Operand::Copy(p)));
                Operand::Move(Place::local(t))
            }
            other => other,
        }
    }

    fn unsupported(&mut self, what: &str, dest: Place, span: Span) {
        self.call(Callee::Intrinsic(Intrinsic::Unsupported(what.to_string())), Vec::new(), dest, span);
    }

    fn lower_try(&mut self, value: Place, ty: &Ty, dest: Place, span: Span) {
        let _ = span;
        let d = self.new_local(Ty::ANKAM, None);
        self.assign(Place::local(d), Rvalue::Discriminant(value.clone()));
        let ok = self.new_block();
        let err = self.new_block();
        self.body.blocks[self.cur as usize].term =
            Term::Switch { discr: Operand::Copy(Place::local(d)), targets: vec![(0, ok)], otherwise: err };
        self.cur = err;
        let (ok_ty, err_ty) = match ty {
            Ty::App(Con::SareThappu, a) => (a[0].clone(), a[1].clone()),
            _ => (Ty::Error, Ty::Error),
        };
        let e = self.by_type(value.field(0), &err_ty);
        self.assign(Place::local(RETURN), Rvalue::Aggregate(Agg::Adt(AdtRef::SareThappu, 1), vec![e]));
        self.emit_cleanup(0);
        self.terminate(Term::Return);
        self.cur = ok;
        let v = self.by_type(value.field(0), &ok_ty);
        self.assign(dest, Rvalue::Use(v));
    }

    fn lower_unary(&mut self, op: UnOp, operand: &'a Expr, ty: &Ty, dest: Place, span: Span) {
        match op {
            UnOp::Neg => {
                if let ExprKind::Int(lit) = &operand.kind {
                    let v = i128::from_str_radix(&lit.digits, lit.radix).unwrap_or(0);
                    return self.assign(dest, Rvalue::Use(Operand::Const(Const::Int(-v))));
                }
                if let ExprKind::Float(text) = &operand.kind {
                    let c = match float_const(text, ty) {
                        Const::F32(x) => Const::F32(-x),
                        Const::F64(x) => Const::F64(-x),
                        other => other,
                    };
                    return self.assign(dest, Rvalue::Use(Operand::Const(c)));
                }
                let a = self.scalar_operand(operand);
                match op_ty(ty) {
                    Some(t) => self.assign(dest, Rvalue::Unary(UnOp::Neg, t, a, span)),
                    None => {
                        let ot = self.ty_of(operand);
                        let p = self.operand_place(a, ot.clone());
                        let r = self.ref_to(p, &ot, false);
                        let fid = self.std_method("Thippochu", "thippu");
                        self.call(Callee::Trait(fid, vec![ot]), vec![r], dest, span);
                    }
                }
            }
            UnOp::Not | UnOp::BitNot => {
                let a = self.scalar_operand(operand);
                let t = op_ty(ty).unwrap_or(OpTy::Bool);
                self.assign(dest, Rvalue::Unary(op, t, a, span));
            }
            UnOp::Deref => {
                let p = self.place_of(operand).deref();
                let op = self.by_type(p, ty);
                self.assign(dest, Rvalue::Use(op));
            }
            UnOp::Ref | UnOp::RefMut => {
                let mutable = op == UnOp::RefMut;
                if let ExprKind::Index { base, indices } = &operand.kind {
                    if let [Expr { kind: ExprKind::Range { lo, hi, inclusive }, .. }] = indices.as_slice() {
                        let id = expr_id(self.file, operand.span);
                        let derefs = self.tc.results.index_derefs.get(&id).copied().unwrap_or(0);
                        let mut b = self.place_of(base);
                        for _ in 0..derefs {
                            b = b.deref();
                        }
                        let lo = self.scalar_operand(lo);
                        let hi = self.scalar_operand(hi);
                        let rv = Rvalue::Slice {
                            place: b,
                            lo,
                            hi,
                            inclusive: *inclusive,
                            mutable,
                            span: operand.span,
                        };
                        return self.assign(dest, rv);
                    }
                }
                let p = self.place_of(operand);
                self.assign(dest, Rvalue::Ref(mutable, p));
            }
        }
    }

    fn std_method(&self, trait_name: &str, method: &str) -> FnId {
        self.g
            .std_trait(trait_name)
            .and_then(|d| self.g.traits.get(&d))
            .and_then(|t| t.methods.iter().find(|(n, _)| n == method).map(|(_, f)| *f))
            .unwrap_or(0)
    }

    /// `a op b` through the operator traits (§10.5.2); operands by reference.
    fn trait_binary(&mut self, op: BinOp, lt: &Ty, a: Place, b: Place, span: Span) -> Rvalue {
        let (tr, m) = match op {
            BinOp::Add => ("Kalupochu", "kalupu"),
            BinOp::Sub => ("Theeseyochu", "theesey"),
            BinOp::Mul => ("Guninchochu", "guninchu"),
            BinOp::Div => ("Panchochu", "panchu"),
            BinOp::Rem => ("Sheshamochu", "shesham"),
            BinOp::MatMul => ("MatmulCheyochu", "matmul"),
            BinOp::Eq | BinOp::Ne => ("Polchochu", "okateyna"),
            _ => ("VarusaPettochu", "polchu"),
        };
        let fid = self.std_method(tr, m);
        let ra = self.ref_to(a, lt, false);
        let rb = self.ref_to(b, lt, false);
        let rty = match op {
            BinOp::Eq | BinOp::Ne => Ty::Bool,
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => Ty::Error,
            _ => lt.clone(),
        };
        let r = self.new_local(rty, None);
        self.call(Callee::Trait(fid, vec![lt.clone()]), vec![ra, rb], Place::local(r), span);
        let rp = Operand::Move(Place::local(r));
        match op {
            BinOp::Ne => Rvalue::Unary(UnOp::Not, OpTy::Bool, rp, span),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                // `polchu` returns `Kramam`: Thakkuva 0, Samaanam 1, Ekkuva 2.
                let d = self.new_local(Ty::ANKAM, None);
                self.assign(Place::local(d), Rvalue::Discriminant(Place::local(r)));
                let (cmp, k) = match op {
                    BinOp::Lt => (BinOp::Eq, 0),
                    BinOp::Le => (BinOp::Ne, 2),
                    BinOp::Gt => (BinOp::Eq, 2),
                    _ => (BinOp::Ne, 0),
                };
                Rvalue::Binary(
                    cmp,
                    OpTy::Int(crate::types::IntTy::ANKAM),
                    Operand::Copy(Place::local(d)),
                    Operand::Const(Const::Int(k)),
                    span,
                )
            }
            _ => Rvalue::Use(rp),
        }
    }

    fn lower_binary(&mut self, op: BinOp, lhs: &'a Expr, rhs: &'a Expr, dest: Place, span: Span) {
        if matches!(op, BinOp::And | BinOp::Or) {
            let a = self.lower_operand(lhs);
            self.assign(dest.clone(), Rvalue::Use(a));
            let rhs_bb = self.new_block();
            let join = self.new_block();
            let (on_true, on_false) = if op == BinOp::And { (rhs_bb, join) } else { (join, rhs_bb) };
            self.body.blocks[self.cur as usize].term = Term::Switch {
                discr: Operand::Copy(dest.clone()),
                targets: vec![(1, on_true)],
                otherwise: on_false,
            };
            self.cur = rhs_bb;
            let b = self.lower_operand(rhs);
            self.assign(dest, Rvalue::Use(b));
            self.goto(join);
            return;
        }
        let mut lt = self.ty_of(lhs);
        if let Ty::Ref(_, inner) = &lt {
            if op_ty(inner).is_some() {
                lt = (**inner).clone();
            }
        }
        match op_ty(&lt) {
            Some(t) => {
                let a = self.scalar_operand(lhs);
                let a = self.stable(a, lt.clone());
                let b = self.scalar_operand(rhs);
                self.assign(dest, Rvalue::Binary(op, t, a, b, span));
            }
            None => {
                let a = self.place_of(lhs);
                let b = self.place_of(rhs);
                let rv = self.trait_binary(op, &lt, a, b, span);
                self.assign(dest, rv);
            }
        }
    }

    // ── calls (§11–12) ─────────────────────────────────────────────────────

    fn lower_call(
        &mut self,
        call: &'a Expr,
        callee: &'a Expr,
        phrases: &'a [Phrase],
        injected: Option<Val>,
        dest: Place,
    ) {
        let id = expr_id(self.file, call.span);
        let tc = self.tc;
        let Some(plan) = tc.results.calls.get(&id) else {
            return self.unsupported("ee call", dest, call.span);
        };
        // Which phrases need a place (borrowed or iterated) rather than a value.
        let mut wants_place: HashSet<ExprId> = HashSet::new();
        let mut note = |a: &ArgRef| {
            if let ArgSrc::Expr(x) | ArgSrc::Spread { expr: x, .. } = &a.src {
                if a.borrow || a.iter.is_some() || matches!(a.src, ArgSrc::Spread { .. }) {
                    wants_place.insert(*x);
                }
            }
        };
        for a in &plan.args {
            match a {
                ArgPlan::One(x) => note(x),
                ArgPlan::Many(xs) => xs.iter().for_each(&mut note),
                ArgPlan::Named(xs) => xs.iter().for_each(|(_, x)| note(x)),
                _ => {}
            }
        }
        // The callee value, then the phrases in source order (§11.5).
        let callee_op = match &plan.target {
            Target::Value => Some(self.lower_operand(callee)),
            _ => None,
        };
        let mut vals: HashMap<ExprId, Val> = HashMap::new();
        for ph in phrases {
            let (PhraseKind::Value(e) | PhraseKind::Spread(e)) = &ph.kind else { continue };
            let eid = expr_id(self.file, e.span);
            let ty = self.ty_of(e);
            let v = if wants_place.contains(&eid) {
                let p = self.place_of(e);
                Val::Place(p, ty)
            } else {
                let op = self.lower_operand(e);
                Val::Op(self.stable_move(op, ty))
            };
            vals.insert(eid, v);
        }
        // The receiver, after the phrases (§13.4.3).
        let mut args = Vec::new();
        let mut pre: Option<Val> = None;
        if let Some(r) = &plan.recv {
            let rt = self.ty_of(&r.expr);
            let mut p = self.place_of(&r.expr);
            let mut t = rt;
            for _ in 0..r.derefs {
                p = p.deref();
                t = match t {
                    Ty::Ref(_, inner) => *inner,
                    Ty::App(Con::Pette, a) => a[0].clone(),
                    other => other,
                };
            }
            let method = match &plan.target {
                Target::Fn { fid, .. } | Target::Trait { fid, .. } => self.g.fns[*fid].receiver.is_some(),
                _ => false,
            };
            if method {
                let op = match r.mode {
                    RecvMode::Ref => self.ref_to(p, &t, false),
                    RecvMode::RefMut => self.ref_to(p, &t, true),
                    RecvMode::Value => self.by_type(p, &t),
                };
                args.push(op);
            } else {
                pre = Some(Val::Place(p, t));
            }
        }
        let callee_fid = match &plan.target {
            Target::Fn { fid, .. } | Target::Trait { fid, .. } => Some(*fid),
            _ => None,
        };
        for (fi, a) in plan.args.iter().enumerate() {
            let op = match a {
                ArgPlan::One(x) => self.arg_operand(x, &vals, injected.as_ref(), pre.as_ref(), call.span),
                ArgPlan::Default => self.default_operand(callee_fid, fi),
                ArgPlan::Missing => Operand::Const(Const::Unit),
                ArgPlan::Many(xs) => {
                    let mut flags = Vec::new();
                    let mut elems = Vec::new();
                    for x in xs {
                        let splice = matches!(x.src, ArgSrc::Spread { index: None, .. });
                        flags.push(splice);
                        elems.push(self.arg_operand(x, &vals, injected.as_ref(), pre.as_ref(), call.span));
                    }
                    let t = self.temp(Ty::Error);
                    self.call(
                        Callee::Intrinsic(Intrinsic::Collect(flags)),
                        elems,
                        Place::local(t),
                        call.span,
                    );
                    self.ref_to(Place::local(t), &Ty::Error, false)
                }
                ArgPlan::Named(xs) => {
                    let mut ops = Vec::new();
                    for (name, x) in xs {
                        ops.push(Operand::Const(Const::Str(Rc::from(name.as_str()))));
                        ops.push(self.arg_operand(x, &vals, injected.as_ref(), pre.as_ref(), call.span));
                    }
                    let t = self.temp(Ty::Error);
                    self.assign(Place::local(t), Rvalue::Aggregate(Agg::Jodi, ops));
                    Operand::Move(Place::local(t))
                }
            };
            args.push(op);
        }
        let callee = match &plan.target {
            Target::Fn { fid, tys } => Callee::Fn(*fid, tys.clone()),
            Target::Trait { fid, tys } => Callee::Trait(*fid, tys.clone()),
            Target::Value => Callee::Value(callee_op.unwrap_or(Operand::Const(Const::Unit))),
            Target::Opaque => Callee::Intrinsic(Intrinsic::Unsupported("torch/python".into())),
        };
        self.call(callee, args, dest, call.span);
    }

    fn stable_move(&mut self, op: Operand, ty: Ty) -> Operand {
        match op {
            Operand::Copy(p) => {
                let t = self.new_local(ty, None);
                self.assign(Place::local(t), Rvalue::Use(Operand::Copy(p)));
                Operand::Move(Place::local(t))
            }
            other => other,
        }
    }

    fn arg_operand(
        &mut self,
        a: &ArgRef,
        vals: &HashMap<ExprId, Val>,
        injected: Option<&Val>,
        pre: Option<&Val>,
        span: Span,
    ) -> Operand {
        let val = match &a.src {
            ArgSrc::Expr(x) => vals.get(x).cloned(),
            ArgSrc::Injected => injected.cloned(),
            ArgSrc::Pre => pre.cloned(),
            ArgSrc::Spread { expr, index } => match (vals.get(expr), index) {
                (Some(Val::Place(p, ty)), Some(i)) => {
                    let et = match ty {
                        Ty::Tuple(items) => items.get(*i as usize).cloned().unwrap_or(Ty::Error),
                        Ty::Array(e, _) => (**e).clone(),
                        _ => Ty::Error,
                    };
                    let field = match ty {
                        Ty::Array(..) => {
                            let il = self.new_local(Ty::ANKAM, None);
                            self.assign(
                                Place::local(il),
                                Rvalue::Use(Operand::Const(Const::Int(*i as i128))),
                            );
                            p.project(Proj::Index(il, span))
                        }
                        _ => p.field(*i),
                    };
                    Some(Val::Place(field, et))
                }
                (Some(Val::Place(p, ty)), None) => {
                    // The whole sequence, spliced by `Collect`.
                    let r = self.ref_to(p.clone(), ty, false);
                    return r;
                }
                (v, _) => v.cloned(),
            },
        };
        let Some(val) = val else { return Operand::Const(Const::Unit) };
        if let Some(item) = &a.iter {
            let (op, ty) = match val {
                Val::Place(p, ty) if owned_collection(&ty) => (self.ref_to(p, &ty, false), ty),
                Val::Place(p, ty) => (self.by_type(p, &ty), ty),
                Val::Op(o) => (o, Ty::Error),
            };
            let it = self.make_iter(op, ty, item, span);
            return Operand::Move(Place::local(it));
        }
        if a.borrow {
            return match val {
                Val::Place(p, ty) => self.ref_to(p, &ty, false),
                Val::Op(o) => {
                    let p = self.operand_place(o, Ty::Error);
                    self.ref_to(p, &Ty::Error, false)
                }
            };
        }
        match val {
            Val::Place(p, ty) => self.by_type(p, &ty),
            Val::Op(o) => o,
        }
    }

    /// A formal's default value (§11.2.6), lowered in the callee's file.
    fn default_operand(&mut self, fid: Option<FnId>, formal: usize) -> Operand {
        let Some(fid) = fid else { return Operand::Const(Const::Unit) };
        let sig = &self.g.fns[fid];
        let Some(f) = sig.params.get(formal) else { return Operand::Const(Const::Unit) };
        let Some(expr) = f.default.as_ref() else { return Operand::Const(Const::Unit) };
        match sig.file {
            None => Operand::Const(literal_const(expr, &f.ty)),
            Some(file) => {
                let saved = self.file;
                self.file = file;
                let op = self.lower_operand(expr);
                let ty = self.ty_of(expr);
                let op = self.stable_move(op, ty);
                self.file = saved;
                op
            }
        }
    }

    // ── closures (§10.7) ───────────────────────────────────────────────────

    fn lower_closure(
        &mut self,
        e: &'a Expr,
        moves: bool,
        params: &'a [ClosureParam],
        body: &'a Expr,
        dest: Place,
    ) {
        let (param_tys, ret) = match self.ty_of(e) {
            Ty::Fn(ps, r) => (ps.into_iter().map(|(t, _)| t).collect::<Vec<_>>(), *r),
            _ => (vec![Ty::Error; params.len()], Ty::Error),
        };
        // What the body uses from outside: locals declared before the closure, and `naa`.
        let mut used: Vec<Option<usize>> = Vec::new();
        self.collect_captures(body, e.span, &mut used);
        let mut caps = Vec::new();
        for u in &used {
            let ty = match u {
                Some(decl) => self.capture_ty(*decl),
                None => self.naa.map(|(l, _)| self.body.locals[l as usize].ty.clone()).unwrap_or(Ty::Error),
            };
            caps.push((*u, ty));
        }
        let name = format!("{}::pani", self.body.name);
        let generics = self.generics.clone();
        let file = self.file;
        let id = {
            let mut b = Builder::new(self.tc, self.cache.clone(), &mut *self.prog, file, generics, name);
            b.new_local(ret, None);
            for (u, ty) in &caps {
                let by_ref = !moves;
                let lt = if by_ref { Ty::reference(false, ty.clone()) } else { ty.clone() };
                let l = b.new_local(lt, None);
                match u {
                    Some(decl) => {
                        b.captures.insert(*decl, (l, by_ref));
                    }
                    None => b.naa = Some((l, by_ref)),
                }
            }
            b.push_frame(false);
            for (p, ty) in params.iter().zip(&param_tys) {
                let l = b.new_local(ty.clone(), Some(p.name.name.clone()));
                b.vars.insert(p.name.span.start, l);
            }
            b.body.params = b.body.locals.len() as u32 - 1;
            b.push_frame(true);
            b.lower_into(body, Place::local(RETURN));
            b.pop_frame();
            b.emit_cleanup(0);
            b.terminate(Term::Return);
            let built = b.into_body();
            self.prog.bodies.push(built);
            (self.prog.bodies.len() - 1) as BodyId
        };
        let mut ops = Vec::new();
        for (u, ty) in caps {
            let p = match u {
                Some(decl) => self.var_place(decl),
                None => self.naa_place(),
            };
            ops.push(if moves { self.by_type(p, &ty) } else { self.ref_to(p, &ty, false) });
        }
        self.assign(dest, Rvalue::Aggregate(Agg::Closure(id), ops));
    }

    fn capture_ty(&self, decl: usize) -> Ty {
        if let Some(&l) = self.vars.get(&decl) {
            return self.body.locals[l as usize].ty.clone();
        }
        match self.captures.get(&decl) {
            Some(&(l, true)) => match &self.body.locals[l as usize].ty {
                Ty::Ref(_, inner) => (**inner).clone(),
                other => other.clone(),
            },
            Some(&(l, false)) => self.body.locals[l as usize].ty.clone(),
            None => self.local_ty(decl),
        }
    }

    fn collect_captures(&self, e: &Expr, within: Span, out: &mut Vec<Option<usize>>) {
        let id = expr_id(self.file, e.span);
        let mut add = |u: Option<usize>| {
            if !out.contains(&u) {
                out.push(u);
            }
        };
        let path_capture = |res: Option<&PathRes>, add: &mut dyn FnMut(Option<usize>)| match res {
            Some(PathRes::Local { decl, .. }) if *decl < within.start || *decl >= within.end => {
                add(Some(*decl))
            }
            Some(PathRes::Naa { .. }) => add(None),
            _ => {}
        };
        match &e.kind {
            ExprKind::Path(_) => path_capture(self.tc.results.paths.get(&id), &mut add),
            ExprKind::Call { callee, phrases, .. } => {
                if let Some(plan) = self.tc.results.calls.get(&id) {
                    if let Some(r) = &plan.recv {
                        self.collect_captures(&r.expr, within, out);
                    }
                    if matches!(plan.target, Target::Value) {
                        self.collect_captures(callee, within, out);
                    }
                }
                for ph in phrases {
                    if let PhraseKind::Value(x) | PhraseKind::Spread(x) = &ph.kind {
                        self.collect_captures(x, within, out);
                    }
                }
            }
            ExprKind::FStr(parts) => {
                for p in parts {
                    if let FStrPart::Expr(x) = p {
                        self.collect_captures(x, within, out);
                    }
                }
            }
            ExprKind::Try(x) | ExprKind::Unary { operand: x, .. } => self.collect_captures(x, within, out),
            ExprKind::Field { base, .. } | ExprKind::TupleIndex { base, .. } => {
                self.collect_captures(base, within, out)
            }
            ExprKind::Pipe { input, stage } => {
                self.collect_captures(input, within, out);
                self.collect_captures(stage, within, out);
            }
            ExprKind::Binary { lhs, rhs, .. } | ExprKind::Range { lo: lhs, hi: rhs, .. } => {
                self.collect_captures(lhs, within, out);
                self.collect_captures(rhs, within, out);
            }
            ExprKind::Index { base, indices } => {
                self.collect_captures(base, within, out);
                for i in indices {
                    self.collect_captures(i, within, out);
                }
            }
            ExprKind::Closure { body, .. } => self.collect_captures(body, within, out),
            ExprKind::Ctor { args: items, .. } | ExprKind::Tuple(items) | ExprKind::List(items) => {
                for x in items {
                    self.collect_captures(x, within, out);
                }
            }
            ExprKind::Record { fields, .. } => {
                for (_, x) in fields {
                    self.collect_captures(x, within, out);
                }
            }
            ExprKind::Map(pairs) => {
                for (k, v) in pairs {
                    self.collect_captures(k, within, out);
                    self.collect_captures(v, within, out);
                }
            }
            _ => {}
        }
    }
}

// ── helpers ────────────────────────────────────────────────────────────────

/// The built-in operator type of a scalar, if it is one (§10.5.1).
pub fn op_ty(ty: &Ty) -> Option<OpTy> {
    Some(match ty {
        Ty::Int(i) => OpTy::Int(*i),
        Ty::Float(b) => OpTy::Float(*b),
        Ty::Bool => OpTy::Bool,
        Ty::Char => OpTy::Char,
        Ty::Str => OpTy::Str,
        Ty::Unit => OpTy::Unit,
        _ => return None,
    })
}

fn owned_collection(ty: &Ty) -> bool {
    matches!(ty, Ty::Array(..) | Ty::App(Con::Varusa | Con::Jodi | Con::GajibijiJodi | Con::Gumpu, _))
}

fn is_place(e: &Expr) -> bool {
    crate::typeck::is_place_expr(e)
}

fn float_const(text: &str, ty: &Ty) -> Const {
    let v: f64 = text.parse().unwrap_or(0.0);
    match ty {
        Ty::Float(32) => Const::F32(text.parse().unwrap_or(v as f32)),
        _ => Const::F64(v),
    }
}

/// A literal (possibly negated) as a constant of type `ty`.
fn literal_const(e: &Expr, ty: &Ty) -> Const {
    match &e.kind {
        ExprKind::Int(lit) => {
            let v = i128::from_str_radix(&lit.digits, lit.radix).unwrap_or(0);
            match ty {
                Ty::Float(32) => Const::F32(v as f32),
                Ty::Float(_) => Const::F64(v as f64),
                _ => Const::Int(v),
            }
        }
        ExprKind::Float(t) => float_const(t, ty),
        ExprKind::Str(s) => Const::Str(Rc::from(s.as_str())),
        ExprKind::Char(c) => match ty {
            Ty::Int(_) => Const::Int(*c as i128),
            _ => Const::Char(*c),
        },
        ExprKind::Bool(b) => Const::Bool(*b),
        ExprKind::Unit => Const::Unit,
        ExprKind::Unary { op: UnOp::Neg, operand } => match literal_const(operand, ty) {
            Const::Int(v) => Const::Int(-v),
            Const::F32(x) => Const::F32(-x),
            Const::F64(x) => Const::F64(-x),
            other => other,
        },
        _ => Const::Unit,
    }
}

fn pattern_names<'p>(p: &'p Pattern, out: &mut Vec<&'p Ident>) {
    match &p.kind {
        PatternKind::Bind { name, .. } => out.push(name),
        PatternKind::Ctor { args, .. } => args.iter().flatten().for_each(|a| pattern_names(a, out)),
        PatternKind::Record { fields, .. } => {
            for (n, sub) in fields {
                match sub {
                    Some(s) => pattern_names(s, out),
                    None => out.push(n),
                }
            }
        }
        PatternKind::Tuple(items) => items.iter().for_each(|a| pattern_names(a, out)),
        PatternKind::Or(alts) => {
            if let Some(a) = alts.first() {
                pattern_names(a, out)
            }
        }
        _ => {}
    }
}

/// Whether dropping a value of this type can run a destructor (`Sardhukochu`).
pub fn needs_drop(
    g: &Globals,
    generics: &[Generic],
    cache: &RefCell<HashMap<Ty, bool>>,
    ty: &Ty,
    visiting: &mut HashSet<DefId>,
) -> bool {
    if let Some(&v) = cache.borrow().get(ty) {
        return v;
    }
    let v = match ty {
        Ty::Param(_) => true,
        Ty::App(Con::Adt(d), args) => {
            if !visiting.insert(*d) {
                return false;
            }
            let has_sardhu = g.std_trait("Sardhukochu").is_some_and(|s| {
                Solver { g, generics }
                    .find_impl(&mut InferTable::default(), ty, &TraitRef { def: s, args: Vec::new() }, 0)
                    .is_some()
            });
            let r = has_sardhu
                || crate::typeck::solve::adt_field_types(g, *d, args)
                    .iter()
                    .any(|f| needs_drop(g, generics, cache, f, visiting));
            visiting.remove(d);
            r
        }
        Ty::App(_, args) | Ty::Tuple(args) => {
            args.iter().any(|a| needs_drop(g, generics, cache, a, visiting))
        }
        Ty::Array(e, _) => needs_drop(g, generics, cache, e, visiting),
        _ => false,
    };
    if !matches!(ty, Ty::Param(_)) {
        cache.borrow_mut().insert(ty.clone(), v);
    }
    v
}
