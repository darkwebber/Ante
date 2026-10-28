//! The borrow checker (spec §15), run on each MIR body after lowering.
//!
//! Two analyses share one forward pass over the control-flow graph:
//! - **moves** (§15.1): which move paths are moved on some path
//!   (`maybe`) and on every path (`must`) to each point;
//! - **holds** (§15.4.2): which loans each local's value may carry.
//!
//! A backward liveness pass gives the locals still used later. A loan is
//! live at a point when a live local holds it, and every access is checked
//! against the live loans with the table of §15.4.4.

use std::collections::{HashMap, HashSet};

use super::*;
use crate::diag::Diagnostic;
use crate::source::SourceFile;
use crate::typeck::{AdtKind, Globals};
use crate::types::Con;

pub fn check(prog: &Program, g: &Globals, sources: &[SourceFile]) -> Vec<(FileId, Diagnostic)> {
    let mut out = Vec::new();
    for body in &prog.bodies {
        let Some(file) = body.file else { continue };
        let mut cx = Cx::new(g, body, sources.get(file));
        cx.run();
        let mut errs = cx.errors;
        errs.sort_by_key(|d| (d.span.start, d.code));
        out.extend(errs.into_iter().map(|d| (file, d)));
    }
    out
}

// ── bit sets ───────────────────────────────────────────────────────────────

/// A bit set that grows on demand; missing words are zero.
#[derive(Clone, PartialEq, Default)]
struct Bits(Vec<u64>);

impl Bits {
    fn new(n: usize) -> Self {
        Bits(vec![0; n.div_ceil(64)])
    }

    fn empty() -> Self {
        Bits(Vec::new())
    }

    fn full(n: usize) -> Self {
        let mut b = Bits(vec![!0; n.div_ceil(64)]);
        if n % 64 != 0 {
            if let Some(last) = b.0.last_mut() {
                *last = (1u64 << (n % 64)) - 1;
            }
        }
        b
    }

    fn get(&self, i: usize) -> bool {
        self.0.get(i / 64).is_some_and(|w| w >> (i % 64) & 1 == 1)
    }

    fn set(&mut self, i: usize, on: bool) {
        if on {
            if self.0.len() <= i / 64 {
                self.0.resize(i / 64 + 1, 0);
            }
            self.0[i / 64] |= 1 << (i % 64);
        } else if let Some(w) = self.0.get_mut(i / 64) {
            *w &= !(1 << (i % 64));
        }
    }

    /// Adds `o`'s bits; true if any were new.
    fn union(&mut self, o: &Bits) -> bool {
        if self.0.len() < o.0.len() {
            self.0.resize(o.0.len(), 0);
        }
        let mut changed = false;
        for (a, b) in self.0.iter_mut().zip(&o.0) {
            changed |= *b & !*a != 0;
            *a |= b;
        }
        changed
    }

    /// Keeps only bits also in `o`; true if any were removed.
    fn intersect(&mut self, o: &Bits) -> bool {
        let mut changed = false;
        for (i, a) in self.0.iter_mut().enumerate() {
            let b = o.0.get(i).copied().unwrap_or(0);
            changed |= *a & !b != 0;
            *a &= b;
        }
        changed
    }

    fn any(&self) -> bool {
        self.0.iter().any(|w| *w != 0)
    }

    fn ones(&self) -> impl Iterator<Item = usize> + '_ {
        self.0
            .iter()
            .enumerate()
            .flat_map(|(w, &bits)| (0..64).filter(move |i| bits >> i & 1 == 1).map(move |i| w * 64 + i))
    }
}

// ── accesses ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Read,
    Move,
    /// Assignment: replaces the value at the place.
    Write,
    /// `&` (false) or `&maare` (true).
    Borrow(bool),
    /// The value is dropped or its storage ends.
    Dead,
}

struct Access {
    place: Place,
    kind: Kind,
}

fn push_place(out: &mut Vec<Access>, place: &Place, kind: Kind) {
    for p in &place.proj {
        if let Proj::Index(l, _) = p {
            out.push(Access { place: Place::local(*l), kind: Kind::Read });
        }
    }
    out.push(Access { place: place.clone(), kind });
}

fn push_operand(out: &mut Vec<Access>, op: &Operand) {
    match op {
        Operand::Copy(p) => push_place(out, p, Kind::Read),
        Operand::Move(p) => push_place(out, p, Kind::Move),
        Operand::Const(_) => {}
    }
}

fn push_rvalue(out: &mut Vec<Access>, rv: &Rvalue) {
    match rv {
        Rvalue::Ref(m, p) => push_place(out, p, Kind::Borrow(*m)),
        Rvalue::Slice { place, lo, hi, mutable, .. } => {
            push_operand(out, lo);
            push_operand(out, hi);
            push_place(out, place, Kind::Borrow(*mutable));
        }
        Rvalue::Discriminant(p) => push_place(out, p, Kind::Read),
        other => other.operands().into_iter().for_each(|o| push_operand(out, o)),
    }
}

fn stmt_accesses(s: &Stmt) -> Vec<Access> {
    let mut out = Vec::new();
    match s {
        Stmt::Assign(p, rv) | Stmt::Replace(p, rv, _) => {
            push_rvalue(&mut out, rv);
            push_place(&mut out, p, Kind::Write);
        }
        Stmt::Drop(p, _) | Stmt::FreeBox(p, _) => out.push(Access { place: p.clone(), kind: Kind::Dead }),
        Stmt::StorageDead(l) => out.push(Access { place: Place::local(*l), kind: Kind::Dead }),
    }
    out
}

fn term_accesses(t: &Term) -> Vec<Access> {
    let mut out = Vec::new();
    match t {
        Term::Switch { discr, .. } => push_operand(&mut out, discr),
        Term::Call { callee, args, dest, .. } => {
            if let Callee::Value(o) = callee {
                push_operand(&mut out, o);
            }
            args.iter().for_each(|a| push_operand(&mut out, a));
            push_place(&mut out, dest, Kind::Write);
        }
        Term::Return => out.push(Access { place: Place::local(RETURN), kind: Kind::Read }),
        Term::Goto(_) | Term::Unreachable => {}
    }
    out
}

/// Whether two places may name overlapping memory (§15.4.3).
fn overlap(a: &Place, b: &Place) -> bool {
    a.local == b.local
        && a.proj.iter().zip(&b.proj).all(|(x, y)| match (x, y) {
            (Proj::Field(i), Proj::Field(j)) | (Proj::Variant(i), Proj::Variant(j)) => i == j,
            _ => true,
        })
}

fn is_prefix(a: &Place, b: &Place) -> bool {
    a.local == b.local && a.proj.len() <= b.proj.len() && a.proj.iter().zip(&b.proj).all(|(x, y)| x == y)
}

// ── the checker ────────────────────────────────────────────────────────────

struct Loan {
    place: Place,
    mutable: bool,
    span: Span,
}

#[derive(Clone)]
struct State {
    /// Move paths moved on some path here.
    maybe: Bits,
    /// Move paths moved on every path here.
    must: Bits,
    /// The loans each live local's value may carry (dead locals are
    /// pruned at block boundaries, as nothing reads them again).
    holds: HashMap<Local, Bits>,
}

impl State {
    fn join(&mut self, o: &State) -> bool {
        let mut changed = self.maybe.union(&o.maybe);
        changed |= self.must.intersect(&o.must);
        for (l, b) in &o.holds {
            changed |= self.holds.entry(*l).or_default().union(b);
        }
        changed
    }
}

struct Cx<'a> {
    g: &'a Globals<'a>,
    body: &'a Body,
    src: Option<&'a SourceFile>,
    loans: Vec<Loan>,
    loan_at: HashMap<(BlockId, usize), usize>,
    /// Move paths: owned places some operand moves out of.
    paths: Vec<Place>,
    path_moves: Vec<Span>,
    /// The move paths of each local.
    paths_of: HashMap<Local, Vec<usize>>,
    /// Each statement's and terminator's accesses, in order.
    stmt_acc: Vec<Vec<Vec<Access>>>,
    term_acc: Vec<Vec<Access>>,
    errors: Vec<Diagnostic>,
    reported: HashSet<(&'static str, usize)>,
    sardhu: Option<DefId>,
    /// Liveness only matters for locals that can hold loans (RC types,
    /// §9.3.1), so it tracks just those: each one's index in the live sets.
    rc_ix: Vec<Option<usize>>,
    n_rc: usize,
}

impl<'a> Cx<'a> {
    fn new(g: &'a Globals<'a>, body: &'a Body, src: Option<&'a SourceFile>) -> Self {
        let mut cx = Cx {
            g,
            body,
            src,
            loans: Vec::new(),
            loan_at: HashMap::new(),
            paths: Vec::new(),
            path_moves: Vec::new(),
            paths_of: HashMap::new(),
            stmt_acc: body.blocks.iter().map(|b| b.stmts.iter().map(stmt_accesses).collect()).collect(),
            term_acc: body.blocks.iter().map(|b| term_accesses(&b.term)).collect(),
            errors: Vec::new(),
            reported: HashSet::new(),
            sardhu: g.std_trait("Sardhukochu"),
            rc_ix: Vec::new(),
            n_rc: 0,
        };
        for l in &body.locals {
            let ix = is_rc(&l.ty).then_some(cx.n_rc);
            cx.n_rc += usize::from(ix.is_some());
            cx.rc_ix.push(ix);
        }
        for (bi, b) in body.blocks.iter().enumerate() {
            for (si, s) in b.stmts.iter().enumerate() {
                let span = b.spans.get(si).copied().unwrap_or_default();
                if let Stmt::Assign(_, rv) | Stmt::Replace(_, rv, _) = s {
                    let lp = match rv {
                        Rvalue::Ref(m, p) => Some((p, *m)),
                        Rvalue::Slice { place, mutable, .. } => Some((place, *mutable)),
                        _ => None,
                    };
                    if let Some((p, mutable)) = lp {
                        cx.loan_at.insert((bi as BlockId, si), cx.loans.len());
                        cx.loans.push(Loan { place: p.clone(), mutable, span });
                    }
                }
                let acc = std::mem::take(&mut cx.stmt_acc[bi][si]);
                for a in &acc {
                    cx.note_path(a, span);
                }
                cx.stmt_acc[bi][si] = acc;
            }
            let acc = std::mem::take(&mut cx.term_acc[bi]);
            for a in &acc {
                cx.note_path(a, b.term_span);
            }
            cx.term_acc[bi] = acc;
        }
        cx
    }

    fn note_path(&mut self, a: &Access, span: Span) {
        if a.kind == Kind::Move
            && self.owned(&a.place).is_ok()
            && !self.local_paths(a.place.local).iter().any(|&i| self.paths[i] == a.place)
        {
            self.paths_of.entry(a.place.local).or_default().push(self.paths.len());
            self.paths.push(a.place.clone());
            self.path_moves.push(span);
        }
    }

    fn ty(&self, p: &Place) -> Ty {
        place_ty(self.g, self.body, p)
    }

    /// Whether a value can be moved out of the place (§15.1.2): `Err`
    /// names the projection that forbids it.
    fn owned(&self, p: &Place) -> Result<(), &'static str> {
        let mut prefix = Place::local(p.local);
        for proj in &p.proj {
            match proj {
                Proj::Index(..) => return Err("index"),
                Proj::Deref if !matches!(self.ty(&prefix), Ty::App(Con::Pette, _)) => return Err("ref"),
                _ => {}
            }
            prefix = prefix.project(proj.clone());
        }
        Ok(())
    }

    /// The first strict prefix of `p` whose type has a destructor.
    fn sardhu_prefix(&self, p: &Place) -> Option<Ty> {
        let s = self.sardhu?;
        let mut prefix = Place::local(p.local);
        for proj in &p.proj {
            let ty = self.ty(&prefix);
            if matches!(ty, Ty::App(Con::Adt(_), _)) {
                let solver = crate::typeck::solve::Solver { g: self.g, generics: &[] };
                let tref = crate::typeck::TraitRef { def: s, args: Vec::new() };
                if solver.find_impl(&mut crate::types::InferTable::default(), &ty, &tref, 0).is_some() {
                    return Some(ty);
                }
            }
            prefix = prefix.project(proj.clone());
        }
        None
    }

    // ── names for messages ─────────────────────────────────────────────────

    fn name(&self, p: &Place) -> String {
        let Some(root) = self.body.locals[p.local as usize].name.clone() else {
            return "ee value".into();
        };
        let mut s = root;
        let mut prefix = Place::local(p.local);
        let mut variant = false;
        for proj in &p.proj {
            match proj {
                Proj::Field(i) => {
                    let ty = self.ty(&prefix);
                    let field = match &ty {
                        Ty::App(Con::Adt(d), _) if !variant => match self.g.adts.get(d).map(|a| &a.kind) {
                            Some(AdtKind::Record(fs)) => fs.get(*i as usize).map(|f| f.name.clone()),
                            _ => None,
                        },
                        _ => None,
                    };
                    s = format!("{s}.{}", field.unwrap_or_else(|| i.to_string()));
                    variant = false;
                }
                Proj::Index(..) => s.push_str("[…]"),
                Proj::Variant(_) => variant = true,
                Proj::Deref => {}
            }
            prefix = prefix.project(proj.clone());
        }
        s
    }

    fn line(&self, span: Span) -> String {
        match self.src {
            Some(src) => src.line_col(span.start).0.to_string(),
            None => "?".into(),
        }
    }

    fn error(&mut self, code: &'static str, span: Span, msg: String) {
        if self.reported.insert((code, span.start)) {
            self.errors.push(Diagnostic::error(code, span, msg));
        }
    }

    // ── liveness (backward) ────────────────────────────────────────────────

    fn live_transfer(&self, live: &mut Bits, accesses: &[Access]) {
        // Writes of whole locals happen last in a statement; kill them first.
        for a in accesses.iter().rev() {
            let Some(ix) = self.rc_ix[a.place.local as usize] else { continue };
            match a.kind {
                Kind::Write if a.place.proj.is_empty() => live.set(ix, false),
                Kind::Write if !a.place.proj.contains(&Proj::Deref) => {}
                Kind::Dead => {}
                _ => live.set(ix, true),
            }
        }
    }

    /// Whether local `l` is in a live set.
    fn is_live(&self, live: &Bits, l: Local) -> bool {
        self.rc_ix[l as usize].is_some_and(|ix| live.get(ix))
    }

    /// The live locals on entry to each block.
    fn liveness(&self) -> Vec<Bits> {
        let n = self.n_rc;
        let blocks = &self.body.blocks;
        let mut live_in: Vec<Bits> = vec![Bits::new(n); blocks.len()];
        let mut changed = true;
        while changed {
            changed = false;
            for bi in (0..blocks.len()).rev() {
                let live = self.block_live(bi, &live_in).swap_remove(0);
                if live != live_in[bi] {
                    live_in[bi] = live;
                    changed = true;
                }
            }
        }
        live_in
    }

    /// The live locals before each statement of a block and before its
    /// terminator (the last entry).
    fn block_live(&self, bi: usize, live_in: &[Bits]) -> Vec<Bits> {
        let b = &self.body.blocks[bi];
        let mut live = Bits::new(self.n_rc);
        for s in b.term.successors() {
            live.union(&live_in[s as usize]);
        }
        let mut per = vec![Bits::empty(); b.stmts.len() + 1];
        self.live_transfer(&mut live, &self.term_acc[bi]);
        per[b.stmts.len()] = live.clone();
        for i in (0..b.stmts.len()).rev() {
            self.live_transfer(&mut live, &self.stmt_acc[bi][i]);
            per[i] = live.clone();
        }
        per
    }

    // ── forward transfer ───────────────────────────────────────────────────

    fn holds_of_place(&self, st: &State, p: &Place) -> Bits {
        st.holds.get(&p.local).cloned().unwrap_or_default()
    }

    fn local_paths(&self, l: Local) -> &[usize] {
        self.paths_of.get(&l).map_or(&[], |v| v.as_slice())
    }

    fn holds_of_ops<'o>(&self, st: &State, ops: impl IntoIterator<Item = &'o Operand>) -> Bits {
        let mut out = Bits::empty();
        for o in ops {
            if let Operand::Copy(p) | Operand::Move(p) = o {
                out.union(&self.holds_of_place(st, p));
            }
        }
        out
    }

    fn assign_holds(&self, st: &mut State, dest: &Place, new: Bits) {
        if dest.proj.is_empty() {
            if new.any() {
                st.holds.insert(dest.local, new);
            } else {
                st.holds.remove(&dest.local);
            }
        } else if new.any() {
            st.holds.entry(dest.local).or_default().union(&new);
        }
    }

    /// The loans a call's result may carry (§9.3.2(c), §21.4).
    fn call_holds(&self, st: &State, callee: &Callee, args: &[Operand], dest: &Place) -> Bits {
        let dest_rc = is_rc(&self.ty(dest));
        let all = || self.holds_of_ops(st, args.iter());
        match callee {
            Callee::Intrinsic(Intrinsic::IntoIter { .. } | Intrinsic::Collect(_)) => all(),
            Callee::Intrinsic(Intrinsic::IterNext) if dest_rc => {
                // An item borrows from what the iterator borrows, not from
                // the iterator itself.
                let mut h = all();
                for i in h.clone().ones() {
                    let l = &self.loans[i].place;
                    let ty = &self.body.locals[l.local as usize].ty;
                    if l.proj.is_empty() && matches!(ty, Ty::App(Con::Pravaham, _)) {
                        h.set(i, false);
                    }
                }
                h
            }
            Callee::Intrinsic(_) => Bits::empty(),
            _ if !dest_rc => Bits::empty(),
            Callee::Value(f) => {
                let rc_args = args.iter().filter(|a| self.op_is_rc(a));
                let mut h = self.holds_of_ops(st, rc_args);
                h.union(&self.holds_of_ops(st, std::iter::once(f)));
                h
            }
            Callee::Fn(..) | Callee::Trait(..) => {
                self.holds_of_ops(st, args.iter().filter(|a| self.op_is_rc(a)))
            }
        }
    }

    fn op_is_rc(&self, o: &Operand) -> bool {
        match o {
            Operand::Copy(p) | Operand::Move(p) => is_rc(&self.ty(p)),
            Operand::Const(_) => false,
        }
    }

    fn transfer_holds_stmt(&self, st: &mut State, loc: (BlockId, usize), s: &Stmt) {
        match s {
            Stmt::Assign(dest, rv) | Stmt::Replace(dest, rv, _) => {
                let mut new = Bits::empty();
                match rv {
                    Rvalue::Ref(_, p) | Rvalue::Slice { place: p, .. } => {
                        if let Some(&l) = self.loan_at.get(&loc) {
                            new.set(l, true);
                        }
                        new.union(&self.holds_of_place(st, p));
                    }
                    Rvalue::Discriminant(_) => {}
                    _ if is_rc(&self.ty(dest)) => new = self.holds_of_ops(st, rv.operands()),
                    _ => {}
                }
                self.assign_holds(st, dest, new);
            }
            Stmt::Drop(p, _) | Stmt::FreeBox(p, _) if p.proj.is_empty() => {
                st.holds.remove(&p.local);
            }
            Stmt::StorageDead(l) => {
                st.holds.remove(l);
            }
            _ => {}
        }
    }

    fn transfer_moves(&self, st: &mut State, accesses: &[Access]) {
        for a in accesses {
            match a.kind {
                Kind::Move => {
                    if let Some(&i) =
                        self.local_paths(a.place.local).iter().find(|&&i| self.paths[i] == a.place)
                    {
                        st.maybe.set(i, true);
                        st.must.set(i, true);
                    }
                }
                Kind::Write => {
                    for &i in self.local_paths(a.place.local) {
                        if is_prefix(&a.place, &self.paths[i]) {
                            st.maybe.set(i, false);
                            st.must.set(i, false);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn transfer_block_end(&self, st: &mut State, bi: usize) {
        let b = &self.body.blocks[bi];
        self.transfer_moves(st, &self.term_acc[bi]);
        if let Term::Call { callee, args, dest, .. } = &b.term {
            let h = self.call_holds(st, callee, args, dest);
            self.assign_holds(st, dest, h);
        }
    }

    fn entry_state(&self) -> State {
        State { maybe: Bits::new(self.paths.len()), must: Bits::new(self.paths.len()), holds: HashMap::new() }
    }

    /// The state on entry to each block (`None` when unreachable).
    fn forward(&self, live_in: &[Bits]) -> Vec<Option<State>> {
        let blocks = &self.body.blocks;
        let mut entry: Vec<Option<State>> = vec![None; blocks.len()];
        if blocks.is_empty() {
            return entry;
        }
        entry[0] = Some(self.entry_state());
        let mut work = vec![0usize];
        let mut queued = vec![false; blocks.len()];
        queued[0] = true;
        while let Some(bi) = work.pop() {
            queued[bi] = false;
            let Some(mut st) = entry[bi].clone() else { continue };
            for (si, s) in blocks[bi].stmts.iter().enumerate() {
                self.transfer_moves(&mut st, &self.stmt_acc[bi][si]);
                self.transfer_holds_stmt(&mut st, (bi as BlockId, si), s);
            }
            self.transfer_block_end(&mut st, bi);
            for succ in blocks[bi].term.successors() {
                let si = succ as usize;
                let mut st = st.clone();
                st.holds.retain(|l, _| self.is_live(&live_in[si], *l));
                let changed = match &mut entry[si] {
                    None => {
                        let mut first = st.clone();
                        // `must` starts at "everything" so the intersection works.
                        first.must.union(&Bits::full(self.paths.len()));
                        first.must.intersect(&st.must);
                        entry[si] = Some(first);
                        true
                    }
                    Some(e) => e.join(&st),
                };
                if changed && !queued[si] {
                    queued[si] = true;
                    work.push(si);
                }
            }
        }
        entry
    }

    // ── checking ───────────────────────────────────────────────────────────

    fn run(&mut self) {
        let live_in = self.liveness();
        let entry = self.forward(&live_in);
        for (bi, start) in entry.into_iter().enumerate() {
            let Some(mut st) = start else { continue };
            let live = self.block_live(bi, &live_in);
            let b = &self.body.blocks[bi];
            for (si, s) in b.stmts.iter().enumerate() {
                let span = b.spans.get(si).copied().unwrap_or_default();
                let acc = std::mem::take(&mut self.stmt_acc[bi][si]);
                let mine = self.loan_at.get(&(bi as BlockId, si)).copied();
                self.check_accesses(&mut st, &acc, &live[si], mine, span);
                self.stmt_acc[bi][si] = acc;
                self.transfer_holds_stmt(&mut st, (bi as BlockId, si), s);
            }
            let acc = std::mem::take(&mut self.term_acc[bi]);
            let span = match &b.term {
                Term::Call { span, .. } => *span,
                _ => b.term_span,
            };
            let n = b.stmts.len();
            self.check_accesses(&mut st, &acc, &live[n], None, span);
            self.term_acc[bi] = acc;
            if matches!(b.term, Term::Return) {
                self.check_return(&st);
            }
        }
    }

    fn live_loans(&self, st: &State, live: &Bits) -> Bits {
        let mut out = Bits::empty();
        for (l, h) in &st.holds {
            if self.is_live(live, *l) {
                out.union(h);
            }
        }
        out
    }

    fn check_accesses(
        &mut self,
        st: &mut State,
        acc: &[Access],
        live: &Bits,
        mine: Option<usize>,
        span: Span,
    ) {
        let loans = self.live_loans(st, live);
        for a in acc {
            self.check_move_state(st, a, span);
            for li in loans.ones() {
                if Some(li) != mine {
                    self.check_conflict(li, a, span);
                }
            }
            self.transfer_moves(st, std::slice::from_ref(a));
        }
    }

    /// E0601/E0603/E0609: the place must be initialized, and a move must be
    /// out of an owned place.
    fn check_move_state(&mut self, st: &State, a: &Access, span: Span) {
        if a.kind == Kind::Dead {
            return;
        }
        if a.kind == Kind::Move && !a.place.proj.is_empty() {
            let owned = self.owned(&a.place);
            let sardhu = if owned.is_ok() { self.sardhu_prefix(&a.place) } else { None };
            let name = if owned.is_err() || sardhu.is_some() { self.name(&a.place) } else { String::new() };
            match owned {
                Err("index") => {
                    return self.error(
                        "E0603",
                        span,
                        format!("`{name}` ni index nunchi bayatiki move cheyyalem; `.xerox()` cheyyi, leka varusa nunchi `theesey()` vaadu"),
                    )
                }
                Err(_) => {
                    return self.error(
                        "E0603",
                        span,
                        format!("`{name}` reference venakaala undhi, adhi nee sontham kaadhu; bayatiki move cheyyalem, `.xerox()` vaadu"),
                    )
                }
                Ok(()) => {}
            }
            if let Some(ty) = sardhu {
                let tn = match ty {
                    Ty::App(Con::Adt(d), _) => self.g.res.defs[d].name.clone(),
                    _ => "ee type".into(),
                };
                return self.error(
                    "E0603",
                    span,
                    format!("`{name}` ni bayatiki theeyalem: `{tn}` ki `sardhu` undhi, adhi motham value ni chusthundhi"),
                );
            }
        }
        for &i in self.local_paths(a.place.local) {
            let p = &self.paths[i];
            let relevant = match a.kind {
                // A write re-initializes the place; only a moved parent is wrong.
                Kind::Write => is_prefix(p, &a.place) && p.proj.len() < a.place.proj.len(),
                _ => is_prefix(p, &a.place) || is_prefix(&a.place, p),
            };
            if !relevant || !st.maybe.get(i) {
                continue;
            }
            let name = self.name(&a.place);
            let line = self.line(self.path_moves[i]);
            if st.must.get(i) {
                self.error(
                    "E0601",
                    span,
                    format!("`{name}` ni already icchesav kada! (line {line} lo move ayyindhi) Malli vaadalevu; rendu chotla kavalante mundhe `.xerox()` cheyyi"),
                );
            } else {
                self.error(
                    "E0609",
                    span,
                    format!("`{name}` konni daarullo already move ayyindhi (line {line}); ikkada adhi undho ledho guarantee ledhu"),
                );
            }
            return;
        }
    }

    /// E0604–E0606: an access against one live loan (§15.4.4).
    fn check_conflict(&mut self, li: usize, a: &Access, span: Span) {
        let loan = &self.loans[li];
        if !overlap(&loan.place, &a.place) {
            return;
        }
        let (mutable, lspan, lplace) = (loan.mutable, loan.span, loan.place.clone());
        let conflict = match a.kind {
            Kind::Read => mutable,
            Kind::Borrow(false) => mutable,
            Kind::Write | Kind::Dead => !self.through_reference(&a.place, &lplace),
            Kind::Move | Kind::Borrow(true) => true,
        };
        if !conflict {
            return;
        }
        let line = self.line(lspan);
        let lname = self.name(&lplace);
        let name = self.name(&a.place);
        let amp = if mutable { "&maare " } else { "&" };
        match a.kind {
            Kind::Read if mutable => self.error(
                "E0605",
                span,
                format!("`{name}` ni ippudu chadhavalevu: line {line} lo `{amp}{lname}` icchesav, aa reference inka vaadakam lo undhi"),
            ),
            Kind::Read => {}
            Kind::Borrow(false) if !mutable => {}
            Kind::Write if self.through_reference(&a.place, &lplace) => {}
            Kind::Move | Kind::Write | Kind::Borrow(_) => {
                let what = match a.kind {
                    Kind::Move => "move cheyyalem",
                    Kind::Write => "maarchalevu",
                    Kind::Borrow(true) => "`&maare` ivvalem",
                    _ => "inko borrow ivvalem",
                };
                let code = if mutable { "E0605" } else { "E0604" };
                self.error(
                    code,
                    span,
                    format!("`{name}` ni ippudu {what}: line {line} lo `{amp}{lname}` icchesav, aa reference inka vaadakam lo undhi"),
                );
            }
            Kind::Dead => {
                if !self.through_reference(&a.place, &lplace) {
                    // An unnamed local is a temporary: it ends with its statement (§15.5).
                    let temp = self.body.locals[lplace.local as usize].name.is_none();
                    let msg = if temp {
                        "ee value oka temporary: statement aipogaane adhi pothundhi, kaani daani reference tharvatha kuda vaadutunnav. Mundhu daanini oka variable lo pettandi (`x ante …`), tharvatha vaadandi".to_string()
                    } else {
                        format!("`{lname}` ki reference ikkada theesukunnav, kaani `{name}` scope aipoyaka kuda aa reference vaadutunnav")
                    };
                    self.error("E0606", lspan, msg);
                }
            }
        }
    }

    /// Whether `loan` is reached from `base` through a reference, so that
    /// replacing or ending `base` itself leaves the borrowed value alone.
    fn through_reference(&self, base: &Place, loan: &Place) -> bool {
        if !is_prefix(base, loan) {
            return false;
        }
        let mut prefix = base.clone();
        for proj in &loan.proj[base.proj.len()..] {
            if *proj == Proj::Deref && matches!(self.ty(&prefix), Ty::Ref(..) | Ty::Ptr(..)) {
                return true;
            }
            prefix = prefix.project(proj.clone());
        }
        false
    }

    /// E0606: a returned value must not borrow from this function's locals.
    fn check_return(&mut self, st: &State) {
        let held = st.holds.get(&RETURN).cloned().unwrap_or_default();
        for li in held.ones() {
            let place = self.loans[li].place.clone();
            if !self.through_reference(&Place::local(place.local), &place) {
                let name = self.name(&place);
                let span = self.loans[li].span;
                self.error(
                    "E0606",
                    span,
                    format!("`{name}` ee function lo local; dhaani reference ni bayatiki ivvalem, function aipogane `{name}` poyindhi"),
                );
            }
        }
    }
}
