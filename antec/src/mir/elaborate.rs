//! Drop elaboration (§19.3), run after the borrow checker.
//!
//! The builder drops whole locals. When a local is moved out of, wholly or
//! in part, that drop has to skip what is gone: each moved path gets a
//! runtime flag (true while it is initialized), and the local's drop opens
//! up into a drop of each part, guarded by its flag. A `Replace` (an
//! assignment that drops the old value) of such a local is opened up the
//! same way. Both back ends then only ever drop initialized values.

use std::cell::RefCell;
use std::collections::HashMap;

use super::*;
use crate::typeck::Globals;
use crate::types::Con;

pub fn elaborate_drops(prog: &mut Program, g: &Globals) {
    let cache = RefCell::new(HashMap::new());
    for body in &mut prog.bodies {
        elaborate_body(body, g, &cache);
    }
}

fn moved_places(body: &Body) -> Vec<Place> {
    let mut out: Vec<Place> = Vec::new();
    let mut note = |o: &Operand| {
        if let Operand::Move(p) = o {
            if !out.contains(p) {
                out.push(p.clone());
            }
        }
    };
    for b in &body.blocks {
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

fn is_prefix(a: &Place, b: &Place) -> bool {
    a.local == b.local && a.proj.len() <= b.proj.len() && a.proj.iter().zip(&b.proj).all(|(x, y)| x == y)
}

struct Elab<'a> {
    g: &'a Globals<'a>,
    cache: &'a RefCell<HashMap<Ty, bool>>,
    /// Tracked move paths and their flags.
    flags: Vec<(Place, Local)>,
    locals: Vec<LocalDecl>,
    blocks: Vec<Block>,
    cur: usize,
    span: Span,
}

impl Elab<'_> {
    fn needs_drop(&self, ty: &Ty) -> bool {
        needs_drop_ty(self.g, &[], self.cache, ty)
    }

    fn flag(&self, p: &Place) -> Option<Local> {
        self.flags.iter().find(|(q, _)| q == p).map(|(_, f)| *f)
    }

    fn tracked(&self, p: &Place) -> bool {
        self.flags.iter().any(|(q, _)| is_prefix(p, q) || is_prefix(q, p))
    }

    fn has_tracked_below(&self, p: &Place) -> bool {
        self.flags.iter().any(|(q, _)| is_prefix(p, q) && q.proj.len() > p.proj.len())
    }

    fn new_local(&mut self, ty: Ty) -> Local {
        self.locals.push(LocalDecl { ty, name: None });
        (self.locals.len() - 1) as Local
    }

    fn new_block(&mut self) -> usize {
        self.blocks.push(Block::new());
        self.blocks.len() - 1
    }

    fn emit(&mut self, s: Stmt) {
        let b = &mut self.blocks[self.cur];
        b.stmts.push(s);
        b.spans.push(self.span);
    }

    fn set_flag(&mut self, f: Local, on: bool) {
        self.emit(Stmt::Assign(Place::local(f), Rvalue::Use(Operand::Const(Const::Bool(on)))));
    }

    fn end(&mut self, t: Term) {
        let b = &mut self.blocks[self.cur];
        b.term = t;
        b.term_span = self.span;
    }

    /// Flags after moving out of the operands.
    fn note_moves<'o>(&mut self, ops: impl IntoIterator<Item = &'o Operand>) {
        for o in ops {
            if let Operand::Move(p) = o {
                if let Some(f) = self.flag(p) {
                    self.set_flag(f, false);
                }
            }
        }
    }

    /// Flags after writing a whole value to `w`.
    fn note_write(&mut self, w: &Place) {
        let fs: Vec<Local> = self.flags.iter().filter(|(q, _)| is_prefix(w, q)).map(|(_, f)| *f).collect();
        for f in fs {
            self.set_flag(f, true);
        }
    }

    /// Drops what is initialized of the value at `p`.
    fn open_drop(&mut self, p: &Place, ty: &Ty) {
        let Some(f) = self.flag(p) else { return self.open_drop_inner(p, ty) };
        let yes = self.new_block();
        let join = self.new_block();
        self.end(Term::Switch {
            discr: Operand::Copy(Place::local(f)),
            targets: vec![(1, yes as BlockId)],
            otherwise: join as BlockId,
        });
        self.cur = yes;
        self.open_drop_inner(p, ty);
        self.end(Term::Goto(join as BlockId));
        self.cur = join;
    }

    fn open_drop_inner(&mut self, p: &Place, ty: &Ty) {
        if !self.has_tracked_below(p) {
            if self.needs_drop(ty) {
                self.emit(Stmt::Drop(p.clone(), ty.clone()));
            }
            return;
        }
        if let Ty::App(Con::Pette, a) = ty {
            self.open_drop(&p.deref(), &a[0].clone());
            self.emit(Stmt::FreeBox(p.clone(), ty.clone()));
            return;
        }
        let variants = variant_fields(self.g, ty);
        if !is_enum(self.g, ty) {
            let fields = variants.into_iter().next().unwrap_or_default();
            for (i, fty) in fields.iter().enumerate().rev() {
                self.open_drop(&p.field(i as u32), fty);
            }
            return;
        }
        // Only the variant some move came out of needs opening up; any
        // other variant is still whole.
        let d = self.new_local(Ty::ANKAM);
        self.emit(Stmt::Assign(Place::local(d), Rvalue::Discriminant(p.clone())));
        let join = self.new_block();
        let whole = self.new_block();
        let mut targets = Vec::new();
        let mut arms = Vec::new();
        for (v, fields) in variants.iter().enumerate() {
            let vp = p.project(Proj::Variant(v as u32));
            if self.has_tracked_below(&vp) {
                let b = self.new_block();
                targets.push((v as i128, b as BlockId));
                arms.push((b, vp, fields.clone()));
            }
        }
        self.end(Term::Switch {
            discr: Operand::Copy(Place::local(d)),
            targets,
            otherwise: whole as BlockId,
        });
        for (b, vp, fields) in arms {
            self.cur = b;
            for (i, fty) in fields.iter().enumerate().rev() {
                self.open_drop(&vp.field(i as u32), fty);
            }
            self.end(Term::Goto(join as BlockId));
        }
        self.cur = whole;
        if self.needs_drop(ty) {
            self.emit(Stmt::Drop(p.clone(), ty.clone()));
        }
        self.end(Term::Goto(join as BlockId));
        self.cur = join;
    }
}

fn elaborate_body(body: &mut Body, g: &Globals, cache: &RefCell<HashMap<Ty, bool>>) {
    let mut flags_for: Vec<Place> = Vec::new();
    for p in moved_places(body) {
        if !needs_drop_ty(g, &[], cache, &place_ty(g, body, &p)) {
            continue;
        }
        let root = Place::local(p.local);
        if !flags_for.contains(&root) {
            flags_for.push(root);
        }
        if !flags_for.contains(&p) {
            flags_for.push(p);
        }
    }
    if flags_for.is_empty() {
        return;
    }
    let old = std::mem::take(&mut body.blocks);
    let mut e = Elab {
        g,
        cache,
        flags: Vec::new(),
        locals: std::mem::take(&mut body.locals),
        blocks: (0..old.len()).map(|_| Block::new()).collect(),
        cur: 0,
        span: Span::default(),
    };
    for p in flags_for {
        let f = e.new_local(Ty::Bool);
        e.flags.push((p, f));
    }
    // Parameters arrive initialized; everything else starts out empty.
    let inits: Vec<(Local, bool)> =
        e.flags.iter().map(|(p, f)| (*f, p.local >= 1 && p.local <= body.params)).collect();
    for (f, on) in inits {
        e.set_flag(f, on);
    }
    for (bi, b) in old.into_iter().enumerate() {
        if bi > 0 {
            e.cur = bi;
        }
        for (s, span) in b.stmts.into_iter().zip(b.spans) {
            e.span = span;
            match s {
                Stmt::Drop(p, ty) if p.proj.is_empty() && e.flag(&p).is_some() => e.open_drop(&p, &ty),
                Stmt::Replace(p, rv, ty) if e.tracked(&p) => {
                    let ops: Vec<Operand> = rv.operands().into_iter().cloned().collect();
                    let tmp = e.new_local(ty.clone());
                    e.emit(Stmt::Assign(Place::local(tmp), rv));
                    e.note_moves(&ops);
                    e.open_drop(&p, &ty);
                    e.emit(Stmt::Assign(p.clone(), Rvalue::Use(Operand::Move(Place::local(tmp)))));
                    e.note_write(&p);
                }
                other => {
                    let (ops, dest): (Vec<Operand>, Option<Place>) = match &other {
                        Stmt::Assign(p, rv) | Stmt::Replace(p, rv, _) => {
                            (rv.operands().into_iter().cloned().collect(), Some(p.clone()))
                        }
                        _ => (Vec::new(), None),
                    };
                    e.emit(other);
                    e.note_moves(&ops);
                    if let Some(d) = dest {
                        e.note_write(&d);
                    }
                }
            }
        }
        e.span = b.term_span;
        match b.term {
            Term::Call { callee, args, dest, next, span } => {
                let mut ops = args.clone();
                if let Callee::Value(o) = &callee {
                    ops.push(o.clone());
                }
                e.note_moves(&ops);
                let tracked_dest = e.flags.iter().any(|(q, _)| is_prefix(&dest, q));
                if tracked_dest {
                    let mid = e.new_block();
                    let d = dest.clone();
                    e.end(Term::Call { callee, args, dest, next: mid as BlockId, span });
                    e.cur = mid;
                    e.note_write(&d);
                    e.end(Term::Goto(next));
                } else {
                    e.end(Term::Call { callee, args, dest, next, span });
                }
            }
            Term::Switch { discr, targets, otherwise } => {
                e.note_moves(std::iter::once(&discr));
                e.end(Term::Switch { discr, targets, otherwise });
            }
            t => e.end(t),
        }
    }
    body.blocks = e.blocks;
    body.locals = e.locals;
}
