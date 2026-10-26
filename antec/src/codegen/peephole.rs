//! A small clean-up over one function's LLVM text before it's written out.
//!
//! Every MIR local is an `alloca`, so straight-line code is full of
//! `store v, ptr %l3` followed by `%v9 = load ptr %l3`. For locals whose
//! address never escapes (they appear only as the pointer of plain loads and
//! stores), a load that follows a store in the same block is replaced by the
//! stored value, and slots that are never read again lose their stores and
//! their `alloca`. LLVM's `mem2reg` does the same at `-O2`; doing it here
//! makes the IR smaller, so `clang` is faster at every level and `-O0`
//! builds run faster too.

use std::collections::{HashMap, HashSet};

/// The `%name` tokens in a line, with their byte ranges.
fn names(line: &str) -> Vec<(usize, usize)> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let start = i;
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'.') {
                i += 1;
            }
            if i > start + 1 {
                out.push((start, i));
            }
        } else {
            i += 1;
        }
    }
    out
}

/// `%x = load T, ptr %l` → (`%x`, `T`, `%l`).
fn as_load(line: &str) -> Option<(&str, &str, &str)> {
    let (dest, rest) = line.trim().split_once(" = load ")?;
    let (ty, ptr) = rest.rsplit_once(", ptr ")?;
    (!ptr.contains(' ') && !ty.contains("volatile")).then_some((dest, ty, ptr))
}

/// `store T v, ptr %l` → (`T v`, `%l`).
fn as_store(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim().strip_prefix("store ")?;
    let (typed, ptr) = rest.rsplit_once(", ptr ")?;
    (!ptr.contains(' ') && !typed.contains("volatile")).then_some((typed, ptr))
}

fn is_label(line: &str) -> bool {
    !line.starts_with(' ') && line.ends_with(':')
}

/// Splits `T v` into the type and the value, where the type may itself
/// contain spaces (`{ i64, i1 } %v3`).
fn split_typed<'l>(typed: &'l str, ty: &str) -> Option<&'l str> {
    typed.strip_prefix(ty)?.strip_prefix(' ')
}

pub(super) fn optimize(func: &str) -> String {
    let lines: Vec<&str> = func.lines().collect();
    // Slots: `%lN = alloca T` / `%fN = alloca i1`.
    let mut slot_ty: HashMap<&str, &str> = HashMap::new();
    for l in &lines {
        if let Some((name, ty)) = l.trim().split_once(" = alloca ") {
            if name.starts_with("%l") || name.starts_with("%f") {
                slot_ty.insert(name, ty);
            }
        }
    }
    // A slot escapes if it's used any other way than as a load/store pointer.
    let mut escaped: HashSet<&str> = HashSet::new();
    for l in &lines {
        let t = l.trim();
        if t.contains(" = alloca ") {
            continue;
        }
        let ptr_use = as_load(t).map(|(_, _, p)| p).or_else(|| as_store(t).map(|(_, p)| p));
        let toks = names(t);
        for &(a, b) in &toks {
            let n = &t[a..b];
            if slot_ty.contains_key(n) {
                // The pointer of a plain load/store is the line's last token.
                let uses = toks.iter().filter(|&&(x, y)| &t[x..y] == n).count();
                let is_target = ptr_use == Some(n) && b == t.len() && uses == 1;
                if !is_target {
                    escaped.insert(n);
                }
            }
        }
    }
    let simple = |n: &str| slot_ty.contains_key(n) && !escaped.contains(n);

    // Forward stores to loads within a block.
    let mut rename: HashMap<String, String> = HashMap::new();
    let resolve = |rename: &HashMap<String, String>, v: &str| -> String {
        let mut v = v.to_string();
        while let Some(next) = rename.get(&v) {
            v = next.clone();
        }
        v
    };
    let mut drop_line = vec![false; lines.len()];
    let mut known: HashMap<&str, (String, String)> = HashMap::new();
    for (i, l) in lines.iter().enumerate() {
        if is_label(l) {
            known.clear();
            continue;
        }
        if let Some((typed, ptr)) = as_store(l) {
            if simple(ptr) {
                let ty = slot_ty[ptr];
                match split_typed(typed, ty) {
                    Some(v) => {
                        known.insert(ptr, (ty.to_string(), resolve(&rename, v)));
                    }
                    None => {
                        known.remove(ptr);
                    }
                }
            }
        } else if let Some((dest, ty, ptr)) = as_load(l) {
            if simple(ptr) {
                if let Some((sty, v)) = known.get(ptr) {
                    if sty == ty {
                        rename.insert(dest.to_string(), v.clone());
                        drop_line[i] = true;
                    }
                }
            }
        }
    }

    // Slots never loaded any more: their stores and allocas go.
    let mut loaded: HashSet<&str> = HashSet::new();
    for (i, l) in lines.iter().enumerate() {
        if !drop_line[i] {
            if let Some((_, _, p)) = as_load(l) {
                loaded.insert(p);
            }
        }
    }
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        let dead = |p: &str| simple(p) && !loaded.contains(p);
        if as_store(t).is_some_and(|(_, p)| dead(p)) {
            drop_line[i] = true;
        }
        if let Some((name, _)) = t.split_once(" = alloca ") {
            if dead(name) {
                drop_line[i] = true;
            }
        }
    }

    let mut out = String::with_capacity(func.len());
    for (i, l) in lines.iter().enumerate() {
        if drop_line[i] {
            continue;
        }
        if rename.is_empty() {
            out.push_str(l);
        } else {
            let mut last = 0;
            for (a, b) in names(l) {
                if let Some(v) = rename.get(&l[a..b]) {
                    out.push_str(&l[last..a]);
                    out.push_str(&resolve(&rename, v));
                    last = b;
                }
            }
            out.push_str(&l[last..]);
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::optimize;

    #[test]
    fn forwards_and_drops_private_slots() {
        let f = "define internal i64 @f(i64 %p1) {\nentry:\n  %l1 = alloca i64\n  %l2 = alloca i64\n  store i64 %p1, ptr %l1\n  br label %bb0\nbb0:\n  %v1 = load i64, ptr %l1\n  %v2 = add i64 %v1, 1\n  store i64 %v2, ptr %l2\n  %v3 = load i64, ptr %l2\n  ret i64 %v3\n}\n";
        let out = optimize(f);
        // %l1 is loaded in another block, so it stays; %l2 disappears.
        assert!(out.contains("%l1 = alloca"), "{out}");
        assert!(!out.contains("%l2"), "{out}");
        assert!(out.contains("ret i64 %v2"), "{out}");
    }

    #[test]
    fn keeps_slots_whose_address_escapes() {
        let f = "define internal void @f() {\nentry:\n  %l1 = alloca i64\n  %l2 = alloca ptr\n  br label %bb0\nbb0:\n  store i64 5, ptr %l1\n  store ptr %l1, ptr %l2\n  call void @g(ptr %l1)\n  %v1 = load i64, ptr %l1\n  ret void\n}\n";
        let out = optimize(f);
        assert!(out.contains("%v1 = load i64, ptr %l1"), "{out}");
        assert!(out.contains("store i64 5, ptr %l1"), "{out}");
    }
}
