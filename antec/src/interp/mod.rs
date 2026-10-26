//! The MIR interpreter (spec Part V): runs a program with the exact
//! semantics of "Ante" — overflow, bounds and division traps, drop order,
//! `vellagane` — so behavior can be tested before native code generation.
//!
//! Memory is a stack of slots, one per local of every active frame, plus a
//! small heap for values the runtime creates on its own. A reference is a
//! slot and a path into the value stored there.

mod native;
mod value;

use std::collections::HashMap;
use std::fmt::Write as _;
use std::rc::Rc;

use crate::mir::*;
use crate::resolve::FileId;
use crate::source::{SourceFile, Span};
use crate::typeck::{FnId, FnOrigin, Typeck};
use crate::types::{Con, IntTy, Ty};

pub use value::Value;
use value::{Addr, Closure, Elem, OrdMap, Root};

/// How a run ended.
pub struct RunOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    /// The trap code and its 1-based position, if the program trapped (§20.3).
    pub trap: Option<(String, Option<FileId>, usize, usize)>,
}

/// Why execution stopped early.
pub enum Halt {
    Trap { code: &'static str, message: String, file: Option<FileId>, span: Span },
    Exit(i32),
}

type R<T> = Result<T, Halt>;

/// The deepest call nesting before T08 (stack overflow), for the default
/// 8 MB `[build] stack`; a bigger stack allows proportionally more.
const MAX_FRAMES: usize = 20_000;

/// How `antec run` runs a program.
#[derive(Clone, Debug, Default)]
pub struct RunOptions {
    /// Write stdout to the real stdout as it fills.
    pub stream: bool,
    /// `[build] stack` in bytes; 0 means the default.
    pub stack: u64,
    /// The program's arguments (`antec run path -- args`), for `system.args()`.
    pub args: Vec<String>,
}

struct Frame {
    body: BodyId,
    base: usize,
    block: BlockId,
    stmt: usize,
    tys: Rc<[Ty]>,
    /// Where the caller wants the return value.
    dest: Option<Addr>,
}

pub struct Machine<'a> {
    prog: &'a Program,
    tc: &'a Typeck<'a>,
    files: &'a [SourceFile],
    slots: Vec<Value>,
    heap: Vec<Value>,
    frames: Vec<Frame>,
    consts: HashMap<usize, Value>,
    /// Standard functions by "owner.name", resolved once.
    natives: HashMap<FnId, String>,
    /// The trait (definition, method name) each trait method belongs to.
    trait_of: HashMap<FnId, (usize, String)>,
    dispatch: HashMap<(FnId, Vec<Ty>), Dispatch>,
    pub stdout: String,
    pub stderr: String,
    /// Write stdout to the real stdout as it fills (for `antec run`).
    pub stream: bool,
    max_frames: usize,
    pub args: Vec<String>,
    started: std::time::Instant,
}

#[derive(Clone)]
enum Dispatch {
    Body(BodyId, Rc<[Ty]>),
    Native(String),
}

pub fn run(prog: &Program, tc: &Typeck, files: &[SourceFile], entry: FnId, opts: &RunOptions) -> RunOutput {
    let mut m = Machine::new(prog, tc, files);
    m.stream = opts.stream;
    m.args = opts.args.clone();
    if opts.stack > 0 {
        let scaled = MAX_FRAMES as u64 * opts.stack / crate::package::MIN_STACK;
        m.max_frames = scaled.clamp(MAX_FRAMES as u64, 10_000_000) as usize;
    }
    let result = m.call_fn(entry, Rc::from(Vec::new()), Vec::new(), Span::default());
    let mut out = RunOutput { stdout: String::new(), stderr: String::new(), exit_code: 0, trap: None };
    match result {
        Ok(Value::Adt(1, fields)) if is_sare_thappu(&tc.globals.fns[entry].ret) => {
            let err_ty = match &tc.globals.fns[entry].ret {
                Ty::App(Con::SareThappu, a) => a[1].clone(),
                _ => Ty::Error,
            };
            let shown = fields.first().map(|v| m.display(v, &err_ty)).unwrap_or(Ok(String::new()));
            match shown {
                Ok(s) => {
                    m.stderr.push_str(&format!("thappu: {s}\n"));
                    out.exit_code = 1;
                }
                Err(h) => m.report(h, &mut out),
            }
        }
        Ok(_) => {}
        Err(h) => m.report(h, &mut out),
    }
    m.flush();
    out.stdout = std::mem::take(&mut m.stdout);
    out.stderr = std::mem::take(&mut m.stderr);
    out
}

fn is_sare_thappu(t: &Ty) -> bool {
    matches!(t, Ty::App(Con::SareThappu, _))
}

/// The value of one module constant, if it evaluates without trapping.
pub fn const_value(prog: &Program, tc: &Typeck, files: &[SourceFile], def: usize) -> Option<Value> {
    let mut m = Machine::new(prog, tc, files);
    m.const_value(def, Span::default()).ok()
}

/// Evaluates every module constant; a trap is E1102 (§16.3).
pub fn eval_consts(
    prog: &Program,
    tc: &Typeck,
    files: &[SourceFile],
) -> Vec<(FileId, crate::diag::Diagnostic)> {
    let mut m = Machine::new(prog, tc, files);
    let mut out = Vec::new();
    let mut defs: Vec<usize> = prog.const_bodies.keys().copied().collect();
    defs.sort_unstable();
    for def in defs {
        if let Err(Halt::Trap { code, message, .. }) = m.const_value(def, Span::default()) {
            let d = &tc.globals.res.defs[def];
            out.push((
                d.file.unwrap_or(0),
                crate::diag::Diagnostic::error(
                    "E1102",
                    d.span,
                    format!("`{}` lekkisthunte kompa munigindhi ({code}): {message}", d.name),
                ),
            ));
        }
    }
    out
}

impl<'a> Machine<'a> {
    fn new(prog: &'a Program, tc: &'a Typeck<'a>, files: &'a [SourceFile]) -> Self {
        let g = &tc.globals;
        let mut natives = HashMap::new();
        for (fid, sig) in g.fns.iter().enumerate() {
            if matches!(sig.origin, FnOrigin::Std | FnOrigin::StdUnsafe) {
                let owner = sig.owner.clone().unwrap_or_else(|| {
                    if sig.module == 0 {
                        "prelude".to_string()
                    } else {
                        g.res.modules[sig.module].name.clone()
                    }
                });
                natives.insert(fid, format!("{owner}.{}", sig.name));
            }
        }
        let mut trait_of = HashMap::new();
        for (&def, tr) in &g.traits {
            for (name, fid) in &tr.methods {
                trait_of.insert(*fid, (def, name.clone()));
            }
        }
        Machine {
            prog,
            tc,
            files,
            slots: Vec::new(),
            heap: Vec::new(),
            frames: Vec::new(),
            consts: HashMap::new(),
            natives,
            trait_of,
            dispatch: HashMap::new(),
            stdout: String::new(),
            stderr: String::new(),
            stream: false,
            max_frames: MAX_FRAMES,
            args: Vec::new(),
            started: std::time::Instant::now(),
        }
    }

    fn report(&mut self, h: Halt, out: &mut RunOutput) {
        match h {
            Halt::Exit(code) => out.exit_code = code,
            Halt::Trap { code, message, file, span } => {
                let (name, line, col) = match file.and_then(|f| self.files.get(f)) {
                    Some(src) => {
                        let (l, c) = src.line_col(span.start);
                        (src.name.clone(), l, c)
                    }
                    None => ("?".to_string(), 0, 0),
                };
                self.flush();
                let _ = writeln!(self.stderr, "kompa munigindhi! {message} ({name}:{line}:{col})");
                out.exit_code = 101;
                out.trap = Some((code.to_string(), file, line, col));
            }
        }
    }

    pub fn flush(&mut self) {
        if self.stream {
            use std::io::Write;
            let mut so = std::io::stdout();
            let _ = so.write_all(self.stdout.as_bytes());
            let _ = so.flush();
            self.stdout.clear();
            let _ = std::io::stderr().write_all(self.stderr.as_bytes());
            self.stderr.clear();
        }
    }

    fn trap(&self, code: &'static str, message: impl Into<String>, span: Span) -> Halt {
        let file = self.frames.last().and_then(|f| self.prog.bodies[f.body as usize].file);
        Halt::Trap { code, message: message.into(), file, span }
    }

    // ── memory ─────────────────────────────────────────────────────────────

    fn root(&self, r: Root) -> &Value {
        match r {
            Root::Stack(i) => &self.slots[i],
            Root::Heap(i) => &self.heap[i],
        }
    }

    fn root_mut(&mut self, r: Root) -> &mut Value {
        match r {
            Root::Stack(i) => &mut self.slots[i],
            Root::Heap(i) => &mut self.heap[i],
        }
    }

    fn read(&self, a: &Addr) -> &Value {
        let mut v = self.root(a.root);
        for e in &a.path {
            v = v.elem(e);
        }
        v
    }

    fn read_mut(&mut self, a: &Addr) -> &mut Value {
        let mut v = self.root_mut(a.root);
        for e in &a.path {
            v = v.elem_mut(e);
        }
        v
    }

    fn alloc(&mut self, v: Value) -> Addr {
        self.heap.push(v);
        Addr { root: Root::Heap(self.heap.len() - 1), path: Vec::new() }
    }

    fn frame(&self) -> &Frame {
        self.frames.last().expect("a frame")
    }

    fn body(&self) -> &'a Body {
        &self.prog.bodies[self.frame().body as usize]
    }

    fn subst(&self, ty: &Ty) -> Ty {
        let tys = &self.frame().tys;
        if tys.is_empty() {
            ty.clone()
        } else {
            ty.subst(tys)
        }
    }

    /// Where a place lives. A `mukka` place yields a slice cursor.
    fn place(&mut self, p: &Place) -> R<Cursor> {
        let base = self.frame().base;
        let mut cur = Cursor::At(Addr { root: Root::Stack(base + p.local as usize), path: Vec::new() });
        for proj in &p.proj {
            cur = match (cur, proj) {
                (Cursor::At(mut a), Proj::Field(i)) => {
                    a.path.push(Elem::Field(*i));
                    Cursor::At(a)
                }
                (Cursor::At(mut a), Proj::Deref) => match self.read(&a) {
                    Value::Ref(target) => Cursor::At(target.clone()),
                    Value::Slice(target, start, len) => Cursor::Slice(target.clone(), *start, *len),
                    Value::Box(_) => {
                        a.path.push(Elem::Box);
                        Cursor::At(a)
                    }
                    Value::Uninit => {
                        return Err(self.trap("T00", "antec bug: uninitialized reference", Span::default()))
                    }
                    _ => Cursor::At(a),
                },
                (Cursor::At(a), Proj::Index(l, span)) => {
                    let i = self.index_value(*l, *span)?;
                    let len = self.read(&a).seq_len().unwrap_or(0);
                    self.bounds(i, len, *span)?;
                    let mut a = a;
                    a.path.push(Elem::Index(i as usize));
                    Cursor::At(a)
                }
                (Cursor::Slice(a, start, len), Proj::Index(l, span)) => {
                    let i = self.index_value(*l, *span)?;
                    self.bounds(i, len, *span)?;
                    let mut a = a;
                    a.path.push(Elem::Index(start + i as usize));
                    Cursor::At(a)
                }
                (c, Proj::Variant(_)) => c,
                (c, _) => c,
            };
        }
        Ok(cur)
    }

    fn addr(&mut self, p: &Place) -> R<Addr> {
        match self.place(p)? {
            Cursor::At(a) => Ok(a),
            Cursor::Slice(a, _, _) => Ok(a),
        }
    }

    fn index_value(&mut self, l: Local, span: Span) -> R<i128> {
        let base = self.frame().base;
        match &self.slots[base + l as usize] {
            Value::Int(i) => Ok(*i),
            _ => Err(self.trap("T03", "index integer kaadhu", span)),
        }
    }

    fn bounds(&self, i: i128, len: usize, span: Span) -> R<()> {
        if i < 0 || i as usize >= len {
            return Err(self.trap("T03", format!("index {i} bayatiki vellindhi (length {len})"), span));
        }
        Ok(())
    }

    fn operand(&mut self, o: &Operand) -> R<Value> {
        Ok(match o {
            Operand::Const(c) => self.constant(c),
            Operand::Copy(p) => match self.place(p)? {
                Cursor::At(a) => self.read(&a).clone(),
                Cursor::Slice(a, s, n) => Value::Slice(a, s, n),
            },
            Operand::Move(p) => match self.place(p)? {
                Cursor::At(a) => std::mem::replace(self.read_mut(&a), Value::Uninit),
                Cursor::Slice(a, s, n) => Value::Slice(a, s, n),
            },
        })
    }

    fn constant(&self, c: &Const) -> Value {
        match c {
            Const::Unit => Value::Unit,
            Const::Bool(b) => Value::Bool(*b),
            Const::Int(i) => Value::Int(*i),
            Const::F32(x) => Value::F32(*x),
            Const::F64(x) => Value::F64(*x),
            Const::Char(c) => Value::Char(*c),
            Const::Str(s) => Value::Str(s.clone()),
            Const::Fn(fid, tys) => {
                let tys: Vec<Ty> = tys.iter().map(|t| self.subst(t)).collect();
                Value::Fn(*fid, Rc::from(tys))
            }
        }
    }

    // ── running ────────────────────────────────────────────────────────────

    /// Calls a function with already-evaluated arguments and runs it to completion.
    pub fn call_fn(&mut self, fid: FnId, tys: Rc<[Ty]>, args: Vec<Value>, span: Span) -> R<Value> {
        let tmp = self.alloc(Value::Uninit);
        self.start_call(Callee::Fn(fid, tys.to_vec()), args, tmp.clone(), span, Some(tys))?;
        Ok(std::mem::replace(self.read_mut(&tmp), Value::Uninit))
    }

    /// Calls a `pani` value (function or closure) to completion.
    pub fn call_value(&mut self, f: &Value, args: Vec<Value>, span: Span) -> R<Value> {
        let tmp = self.alloc(Value::Uninit);
        let depth = self.frames.len();
        match f {
            Value::Fn(fid, tys) => {
                self.start_call(Callee::Fn(*fid, tys.to_vec()), args, tmp.clone(), span, Some(tys.clone()))?
            }
            Value::Closure(c) => {
                let mut all = c.captures.clone();
                all.extend(args);
                self.push_frame(c.body, c.tys.clone(), all, Some(tmp.clone()), span)?;
                self.run_until(depth)?;
            }
            _ => return Err(self.trap("T00", "antec bug: call of a non-function", span)),
        }
        Ok(std::mem::replace(self.read_mut(&tmp), Value::Uninit))
    }

    /// Calls a callee to completion, writing the result to `dest`.
    fn start_call(
        &mut self,
        callee: Callee,
        args: Vec<Value>,
        dest: Addr,
        span: Span,
        tys: Option<Rc<[Ty]>>,
    ) -> R<()> {
        let depth = self.frames.len();
        if let Some(v) = self.invoke(callee, args, dest.clone(), span, tys)? {
            *self.read_mut(&dest) = v;
        }
        self.run_until(depth)
    }

    fn push_frame(
        &mut self,
        body: BodyId,
        tys: Rc<[Ty]>,
        args: Vec<Value>,
        dest: Option<Addr>,
        span: Span,
    ) -> R<()> {
        if self.frames.len() >= self.max_frames {
            return Err(self.trap("T08", "stack nindipoyindhi (chaala lothu recursion)", span));
        }
        let b = &self.prog.bodies[body as usize];
        let base = self.slots.len();
        self.slots.resize(base + b.locals.len(), Value::Uninit);
        for (i, v) in args.into_iter().enumerate() {
            if let Some(slot) = self.slots.get_mut(base + 1 + i) {
                *slot = v;
            }
        }
        self.frames.push(Frame { body, base, block: 0, stmt: 0, tys, dest });
        Ok(())
    }

    /// Runs frames until the stack is back to `depth` frames.
    fn run_until(&mut self, depth: usize) -> R<()> {
        while self.frames.len() > depth {
            self.step()?;
        }
        Ok(())
    }

    fn step(&mut self) -> R<()> {
        let body = self.body();
        let (block, stmt) = (self.frame().block, self.frame().stmt);
        let b = &body.blocks[block as usize];
        if stmt < b.stmts.len() {
            self.frames.last_mut().expect("frame").stmt += 1;
            return self.exec(&b.stmts[stmt]);
        }
        match &b.term {
            Term::Goto(t) => self.jump(*t),
            Term::Switch { discr, targets, otherwise } => {
                let v = match self.operand(discr)? {
                    Value::Int(i) => i,
                    Value::Bool(b) => b as i128,
                    Value::Char(c) => c as i128,
                    _ => -1,
                };
                let t = targets.iter().find(|(k, _)| *k == v).map_or(*otherwise, |(_, t)| *t);
                self.jump(t);
            }
            Term::Return => {
                let f = self.frames.pop().expect("frame");
                let v = std::mem::replace(&mut self.slots[f.base], Value::Uninit);
                self.slots.truncate(f.base);
                if let Some(dest) = f.dest {
                    *self.read_mut(&dest) = v;
                }
            }
            Term::Unreachable => {
                return Err(self.trap("T00", "antec bug: unreachable code ki vacchindhi", Span::default()))
            }
            Term::Call { callee, args, dest, next, span } => {
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    vals.push(self.operand(a)?);
                }
                let dest = self.addr(dest)?;
                self.jump(*next);
                if let Some(v) = self.invoke(callee.clone(), vals, dest.clone(), *span, None)? {
                    *self.read_mut(&dest) = v;
                }
            }
        }
        Ok(())
    }

    fn jump(&mut self, t: BlockId) {
        let f = self.frames.last_mut().expect("frame");
        f.block = t;
        f.stmt = 0;
    }

    /// Starts a call. A body call pushes a frame (its result arrives at
    /// `dest` on return) and yields `None`; a native call yields its value.
    fn invoke(
        &mut self,
        callee: Callee,
        args: Vec<Value>,
        dest: Addr,
        span: Span,
        tys: Option<Rc<[Ty]>>,
    ) -> R<Option<Value>> {
        match callee {
            Callee::Fn(fid, ts) => {
                let tys: Rc<[Ty]> = match tys {
                    Some(t) => t,
                    None => Rc::from(ts.iter().map(|t| self.subst(t)).collect::<Vec<_>>()),
                };
                self.invoke_fn(fid, tys, args, dest, span)
            }
            Callee::Trait(fid, ts) => {
                let tys: Vec<Ty> = ts.iter().map(|t| self.subst(t)).collect();
                match self.resolve_trait(fid, &tys, span)? {
                    Dispatch::Body(body, btys) => {
                        self.push_frame(body, btys, args, Some(dest), span)?;
                        Ok(None)
                    }
                    Dispatch::Native(name) => self.trait_native(&name, &tys, args, span).map(Some),
                }
            }
            Callee::Value(op) => {
                let f = self.operand(&op)?;
                match f {
                    Value::Fn(fid, tys) => self.invoke_fn(fid, tys, args, dest, span),
                    Value::Closure(c) => {
                        let mut all = c.captures.clone();
                        all.extend(args);
                        self.push_frame(c.body, c.tys.clone(), all, Some(dest), span)?;
                        Ok(None)
                    }
                    _ => Err(self.trap("T00", "antec bug: call of a non-function", span)),
                }
            }
            Callee::Intrinsic(i) => self.intrinsic(&i, args, span).map(Some),
        }
    }

    fn invoke_fn(
        &mut self,
        fid: FnId,
        tys: Rc<[Ty]>,
        args: Vec<Value>,
        dest: Addr,
        span: Span,
    ) -> R<Option<Value>> {
        if let Some(&body) = self.prog.fn_bodies.get(&fid) {
            self.push_frame(body, tys, args, Some(dest), span)?;
            return Ok(None);
        }
        if let Some(name) = self.natives.get(&fid).cloned() {
            return self.native(&name, &tys, args, span).map(Some);
        }
        if self.trait_of.contains_key(&fid) {
            // A trait method called directly with its `idhi` known.
            let tys_v = tys.to_vec();
            return match self.resolve_trait(fid, &tys_v, span)? {
                Dispatch::Body(body, btys) => {
                    self.push_frame(body, btys, args, Some(dest), span)?;
                    Ok(None)
                }
                Dispatch::Native(name) => self.trait_native(&name, &tys_v, args, span).map(Some),
            };
        }
        let name = self.tc.globals.fns[fid].name.clone();
        Err(self.trap("T05", format!("`{name}` ni interpreter lo run cheyyalem (C function)"), span))
    }

    /// Finds the code behind a trait method for concrete types (§13.3).
    fn resolve_trait(&mut self, fid: FnId, tys: &[Ty], span: Span) -> R<Dispatch> {
        let key = (fid, tys.to_vec());
        if let Some(d) = self.dispatch.get(&key) {
            return Ok(d.clone());
        }
        let d = match crate::mir::resolve_trait(self.prog, &self.tc.globals, fid, tys) {
            Some(Resolved::Body(body, tys)) => Dispatch::Body(body, Rc::from(tys)),
            Some(Resolved::Native(name)) => Dispatch::Native(name),
            None => return Err(self.trap("T00", "antec bug: unknown trait method", span)),
        };
        self.dispatch.insert(key, d.clone());
        Ok(d)
    }

    // ── statements ─────────────────────────────────────────────────────────

    fn exec(&mut self, s: &Stmt) -> R<()> {
        match s {
            Stmt::Assign(p, rv) => {
                let v = self.rvalue(rv)?;
                let a = self.addr(p)?;
                *self.read_mut(&a) = v;
            }
            Stmt::Replace(p, rv, ty) => {
                let v = self.rvalue(rv)?;
                let a = self.addr(p)?;
                let old = std::mem::replace(self.read_mut(&a), v);
                let ty = self.subst(ty);
                self.drop_value(old, &ty)?;
            }
            Stmt::Drop(p, ty) => {
                let a = self.addr(p)?;
                let old = std::mem::replace(self.read_mut(&a), Value::Uninit);
                let ty = self.subst(ty);
                self.drop_value(old, &ty)?;
            }
            Stmt::FreeBox(p, _) => {
                // The contents were moved out; only the box itself goes.
                let a = self.addr(p)?;
                *self.read_mut(&a) = Value::Uninit;
            }
            Stmt::StorageDead(_) => {}
        }
        Ok(())
    }

    /// Runs destructors for a value going out of scope (§19.3).
    pub fn drop_value(&mut self, v: Value, ty: &Ty) -> R<()> {
        if matches!(v, Value::Uninit) {
            return Ok(());
        }
        match ty {
            Ty::App(Con::Adt(d), args) => {
                let mut v = v;
                if let Some(s) = self.tc.globals.std_trait("Sardhukochu") {
                    let solver = crate::typeck::solve::Solver { g: &self.tc.globals, generics: &[] };
                    let tref = crate::typeck::TraitRef { def: s, args: Vec::new() };
                    if solver.find_impl(&mut crate::types::InferTable::default(), ty, &tref, 0).is_some() {
                        let complete = match &v {
                            Value::Adt(_, fields) => fields.iter().all(|f| !matches!(f, Value::Uninit)),
                            _ => true,
                        };
                        if complete {
                            let fid = self.tc.globals.traits[&s].methods[0].1;
                            let slot = self.alloc(v);
                            let r = Value::Ref(slot.clone());
                            self.call_trait(fid, vec![ty.clone()], vec![r], Span::default())?;
                            v = std::mem::replace(self.read_mut(&slot), Value::Uninit);
                        }
                    }
                }
                let field_tys = match self.tc.globals.adts.get(d).map(|a| &a.kind) {
                    Some(crate::typeck::AdtKind::Record(fs)) => fs.iter().map(|f| f.ty.subst(args)).collect(),
                    Some(crate::typeck::AdtKind::Enum(vs)) => match &v {
                        Value::Adt(i, _) => vs
                            .get(*i as usize)
                            .map(|var| var.fields.iter().map(|f| f.subst(args)).collect())
                            .unwrap_or_default(),
                        _ => Vec::new(),
                    },
                    None => Vec::new(),
                };
                if let Value::Adt(_, fields) = v {
                    for (f, t) in fields.into_iter().zip(field_tys).rev() {
                        self.drop_value(f, &t)?;
                    }
                }
            }
            Ty::App(Con::Undochu | Con::SareThappu, args) => {
                if let Value::Adt(i, mut fields) = v {
                    let t = if i == 0 { args[0].clone() } else { args.get(1).cloned().unwrap_or(Ty::Unit) };
                    if let Some(f) = fields.pop() {
                        self.drop_value(f, &t)?;
                    }
                }
            }
            Ty::App(Con::Varusa | Con::Pette, _) | Ty::Array(..) => {
                let elem = match ty {
                    Ty::Array(e, _) => (**e).clone(),
                    Ty::App(_, a) => a[0].clone(),
                    _ => Ty::Error,
                };
                match v {
                    Value::Vec(items) | Value::Array(items) => {
                        for x in items {
                            self.drop_value(x, &elem)?;
                        }
                    }
                    Value::Box(inner) => self.drop_value(*inner, &elem)?,
                    _ => {}
                }
            }
            Ty::Tuple(items) => {
                if let Value::Tuple(vals) = v {
                    for (x, t) in vals.into_iter().zip(items.iter()).rev() {
                        self.drop_value(x, t)?;
                    }
                }
            }
            Ty::App(Con::Jodi | Con::GajibijiJodi, args) => {
                if let Value::Map(m) = v {
                    for (k, x) in m.into_entries() {
                        self.drop_value(k, &args[0])?;
                        self.drop_value(x, &args[1])?;
                    }
                }
            }
            Ty::Fn(..) => {
                // The last copy of a closure drops its captures, last first (§10.7.4).
                if let Value::Closure(c) = v {
                    if let Ok(c) = Rc::try_unwrap(c) {
                        let locals = &self.prog.bodies[c.body as usize].locals;
                        let tys: Vec<Ty> =
                            (1..=c.captures.len()).map(|i| locals[i].ty.subst(&c.tys)).collect();
                        for (x, t) in c.captures.into_iter().zip(tys).rev() {
                            self.drop_value(x, &t)?;
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Calls a trait method with concrete types to completion.
    pub fn call_trait(&mut self, fid: FnId, tys: Vec<Ty>, args: Vec<Value>, span: Span) -> R<Value> {
        match self.resolve_trait(fid, &tys, span)? {
            Dispatch::Body(body, btys) => {
                let tmp = self.alloc(Value::Uninit);
                let depth = self.frames.len();
                self.push_frame(body, btys, args, Some(tmp.clone()), span)?;
                self.run_until(depth)?;
                Ok(std::mem::replace(self.read_mut(&tmp), Value::Uninit))
            }
            Dispatch::Native(name) => self.trait_native(&name, &tys, args, span),
        }
    }

    // ── rvalues ────────────────────────────────────────────────────────────

    fn rvalue(&mut self, rv: &Rvalue) -> R<Value> {
        Ok(match rv {
            Rvalue::Use(o) => self.operand(o)?,
            Rvalue::Ref(_, p) => match self.place(p)? {
                Cursor::At(a) => Value::Ref(a),
                Cursor::Slice(a, s, n) => Value::Slice(a, s, n),
            },
            Rvalue::Slice { place, lo, hi, inclusive, span, .. } => {
                let lo = self.int(lo)?;
                let hi = self.int(hi)? + i128::from(*inclusive);
                let (a, start, len) = match self.place(place)? {
                    Cursor::At(a) => {
                        let len = self.read(&a).seq_len().unwrap_or(0);
                        (a, 0, len)
                    }
                    Cursor::Slice(a, s, n) => (a, s, n),
                };
                if lo < 0 || lo > hi || hi as usize > len {
                    return Err(self.trap(
                        "T10",
                        format!("mukka haddulu tappu: {lo} nunchi {hi} (length {len})"),
                        *span,
                    ));
                }
                Value::Slice(a, start + lo as usize, (hi - lo) as usize)
            }
            Rvalue::Binary(op, t, a, b, span) => {
                let x = self.operand(a)?;
                let y = self.operand(b)?;
                self.binary(*op, *t, x, y, *span)?
            }
            Rvalue::Unary(op, t, a, span) => {
                let x = self.operand(a)?;
                self.unary(*op, *t, x, *span)?
            }
            Rvalue::Aggregate(agg, ops) => {
                let mut vals = Vec::with_capacity(ops.len());
                for o in ops {
                    vals.push(self.operand(o)?);
                }
                match agg {
                    Agg::Tuple => Value::Tuple(vals),
                    Agg::Array => Value::Array(vals),
                    Agg::Varusa => Value::Vec(vals),
                    Agg::Pette => Value::Box(Box::new(vals.pop().unwrap_or(Value::Unit))),
                    Agg::Adt(_, v) => Value::Adt(*v, vals),
                    Agg::Jodi => {
                        let mut m = OrdMap::default();
                        let mut it = vals.into_iter();
                        while let (Some(k), Some(v)) = (it.next(), it.next()) {
                            m.insert(k, v);
                        }
                        Value::Map(Box::new(m))
                    }
                    Agg::Closure(body) => Value::Closure(Rc::new(Closure {
                        body: *body,
                        captures: vals,
                        tys: self.frame().tys.clone(),
                    })),
                }
            }
            Rvalue::Discriminant(p) => match self.place(p)? {
                Cursor::At(a) => match self.read(&a) {
                    Value::Adt(v, _) => Value::Int(*v as i128),
                    _ => Value::Int(0),
                },
                Cursor::Slice(..) => Value::Int(0),
            },
            Rvalue::Range(a, b, inclusive, it) => {
                let lo = self.int(a)?;
                let hi = self.int(b)?;
                Value::Range(lo, hi, *inclusive, *it)
            }
        })
    }

    fn int(&mut self, o: &Operand) -> R<i128> {
        match self.operand(o)? {
            Value::Int(i) => Ok(i),
            _ => Ok(0),
        }
    }

    pub fn overflow(&self, it: IntTy, what: impl std::fmt::Display, span: Span) -> Halt {
        let ty = int_name(it);
        self.trap("T01", format!("`{ty}` lo {what} pattadhu"), span)
    }

    pub fn fit(&self, it: IntTy, v: i128, what: impl std::fmt::Display, span: Span) -> R<Value> {
        let (lo, hi) = it.range();
        if v < lo || v > hi {
            return Err(self.overflow(it, what, span));
        }
        Ok(Value::Int(v))
    }

    fn binary(&mut self, op: crate::ast::BinOp, t: OpTy, x: Value, y: Value, span: Span) -> R<Value> {
        use crate::ast::BinOp::*;
        Ok(match (t, x, y) {
            (OpTy::Int(it), Value::Int(a), Value::Int(b)) => match op {
                Add => self.fit(it, a + b, format_args!("{a} + {b}"), span)?,
                Sub => self.fit(it, a - b, format_args!("{a} - {b}"), span)?,
                Mul => match a.checked_mul(b) {
                    Some(v) => self.fit(it, v, format_args!("{a} * {b}"), span)?,
                    None => return Err(self.overflow(it, format_args!("{a} * {b}"), span)),
                },
                Div | Rem => {
                    if b == 0 {
                        return Err(self.trap("T02", "sunna (0) tho bhaagham cheyyalem", span));
                    }
                    if it.signed && b == -1 && a == it.range().0 {
                        let sym = if op == Div { "/" } else { "%" };
                        return Err(self.overflow(it, format_args!("{a} {sym} -1"), span));
                    }
                    Value::Int(if op == Div { a / b } else { a % b })
                }
                Shl | Shr => {
                    if b < 0 || b >= i128::from(it.bits) {
                        return Err(self.trap(
                            "T04",
                            format!("shift count {b} tappu (0..{} undali)", it.bits),
                            span,
                        ));
                    }
                    Value::Int(if op == Shl { wrap(it, a << b) } else { a >> b })
                }
                BitAnd => Value::Int(a & b),
                BitOr => Value::Int(a | b),
                BitXor => Value::Int(a ^ b),
                _ => Value::Bool(compare(op, a.cmp(&b))),
            },
            (OpTy::Float(32), Value::F32(a), Value::F32(b)) => match op {
                Add => Value::F32(a + b),
                Sub => Value::F32(a - b),
                Mul => Value::F32(a * b),
                Div => Value::F32(a / b),
                Rem => Value::F32(a % b),
                _ => Value::Bool(fcompare(op, f64::from(a), f64::from(b))),
            },
            (OpTy::Float(_), Value::F64(a), Value::F64(b)) => match op {
                Add => Value::F64(a + b),
                Sub => Value::F64(a - b),
                Mul => Value::F64(a * b),
                Div => Value::F64(a / b),
                Rem => Value::F64(a % b),
                _ => Value::Bool(fcompare(op, a, b)),
            },
            (OpTy::Str, Value::Str(a), Value::Str(b)) => match op {
                Add => Value::Str(Rc::from(format!("{a}{b}"))),
                _ => Value::Bool(compare(op, a.cmp(&b))),
            },
            (OpTy::Char, Value::Char(a), Value::Char(b)) => Value::Bool(compare(op, a.cmp(&b))),
            (OpTy::Bool, Value::Bool(a), Value::Bool(b)) => match op {
                BitAnd => Value::Bool(a & b),
                BitOr => Value::Bool(a | b),
                BitXor => Value::Bool(a ^ b),
                _ => Value::Bool(compare(op, a.cmp(&b))),
            },
            (OpTy::Unit, _, _) => Value::Bool(matches!(op, Eq | Le | Ge)),
            (_, x, y) => {
                return Err(self.trap("T00", format!("antec bug: operator {op:?} on {x:?}, {y:?}"), span));
            }
        })
    }

    fn unary(&mut self, op: crate::ast::UnOp, t: OpTy, x: Value, span: Span) -> R<Value> {
        use crate::ast::UnOp::*;
        Ok(match (op, t, x) {
            (Neg, OpTy::Int(it), Value::Int(a)) => self.fit(it, -a, format_args!("-({a})"), span)?,
            (Neg, _, Value::F32(a)) => Value::F32(-a),
            (Neg, _, Value::F64(a)) => Value::F64(-a),
            (Not, _, Value::Bool(b)) => Value::Bool(!b),
            (BitNot, OpTy::Int(it), Value::Int(a)) => Value::Int(wrap(it, !a)),
            (BitNot, _, Value::Bool(b)) => Value::Bool(!b),
            (_, _, x) => return Err(self.trap("T00", format!("antec bug: unary {op:?} on {x:?}"), span)),
        })
    }

    // ── constants ──────────────────────────────────────────────────────────

    fn const_value(&mut self, def: usize, span: Span) -> R<Value> {
        if let Some(v) = self.consts.get(&def) {
            return Ok(v.clone());
        }
        let v = match self.prog.const_bodies.get(&def) {
            Some(&body) => {
                let tmp = self.alloc(Value::Uninit);
                let depth = self.frames.len();
                self.push_frame(body, Rc::from(Vec::new()), Vec::new(), Some(tmp.clone()), span)?;
                self.run_until(depth)?;
                std::mem::replace(self.read_mut(&tmp), Value::Uninit)
            }
            None => native::std_const(&self.tc.globals.res.defs[def].name),
        };
        self.consts.insert(def, v.clone());
        Ok(v)
    }
}

enum Cursor {
    At(Addr),
    Slice(Addr, usize, usize),
}

/// A type's name for runtime messages.
pub fn show_ty(t: &Ty) -> String {
    match t {
        Ty::Int(it) => int_name(*it),
        Ty::Float(b) => format!("amsam{b}"),
        Ty::Char => "aksharam".into(),
        other => format!("{other:?}"),
    }
}

pub fn int_name(it: IntTy) -> String {
    format!("{}ankam{}", if it.signed { "" } else { "aasajeevi " }, it.bits)
}

/// Keeps the low `bits` bits, as a value of the type (two's complement).
pub fn wrap(it: IntTy, v: i128) -> i128 {
    let bits = u32::from(it.bits);
    let mask: i128 = if bits >= 128 { -1 } else { (1i128 << bits) - 1 };
    let low = v & mask;
    if it.signed && bits < 128 && low >> (bits - 1) & 1 == 1 {
        low - (1i128 << bits)
    } else {
        low
    }
}

fn compare(op: crate::ast::BinOp, o: std::cmp::Ordering) -> bool {
    use crate::ast::BinOp::*;
    use std::cmp::Ordering::*;
    match op {
        Eq => o == Equal,
        Ne => o != Equal,
        Lt => o == Less,
        Le => o != Greater,
        Gt => o == Greater,
        Ge => o != Less,
        _ => false,
    }
}

fn fcompare(op: crate::ast::BinOp, a: f64, b: f64) -> bool {
    use crate::ast::BinOp::*;
    match op {
        Eq => a == b,
        Ne => a != b,
        Lt => a < b,
        Le => a <= b,
        Gt => a > b,
        Ge => a >= b,
        _ => false,
    }
}
