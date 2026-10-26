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

/// One line of a function, classified once.
#[derive(Clone, Copy)]
enum Line<'a> {
    /// `%x = load T, ptr %l`
    Load {
        dest: &'a str,
        ty: &'a str,
        ptr: &'a str,
    },
    /// `store T v, ptr %l` (`typed` is `T v`)
    Store {
        typed: &'a str,
        ptr: &'a str,
    },
    /// `%l = alloca T`
    Alloca {
        name: &'a str,
        ty: &'a str,
    },
    Label,
    Other,
}

/// `%x = load T, ptr %l` and friends. Plain prefix checks: this runs on
/// every line of every function, so no substring searching.
fn classify(line: &str) -> Line<'_> {
    if !line.starts_with(' ') {
        return if line.ends_with(':') { Line::Label } else { Line::Other };
    }
    let t = line.trim();
    if let Some(rest) = t.strip_prefix("store ") {
        if let Some((typed, ptr)) = split_last_ptr(rest) {
            if !typed.starts_with("volatile") {
                return Line::Store { typed, ptr };
            }
        }
        return Line::Other;
    }
    if t.starts_with('%') {
        let Some(eq) = t.find(" = ") else { return Line::Other };
        let (dest, rest) = (&t[..eq], &t[eq + 3..]);
        if let Some(rest) = rest.strip_prefix("load ") {
            if let Some((ty, ptr)) = split_last_ptr(rest) {
                if !ty.starts_with("volatile") {
                    return Line::Load { dest, ty, ptr };
                }
            }
        } else if let Some(ty) = rest.strip_prefix("alloca ") {
            return Line::Alloca { name: dest, ty };
        }
    }
    Line::Other
}

/// `X, ptr %p` → (`X`, `%p`) when the pointer is a plain name.
fn split_last_ptr(rest: &str) -> Option<(&str, &str)> {
    let i = rest.rfind(", ptr ")?;
    let ptr = &rest[i + 6..];
    (ptr.starts_with('%') && !ptr.contains(' ')).then_some((&rest[..i], ptr))
}

/// Splits `T v` into the type and the value, where the type may itself
/// contain spaces (`{ i64, i1 } %v3`).
fn split_typed<'l>(typed: &'l str, ty: &str) -> Option<&'l str> {
    typed.strip_prefix(ty)?.strip_prefix(' ')
}

pub(super) fn optimize(func: &str) -> String {
    let lines: Vec<&str> = func.lines().collect();
    let kinds: Vec<Line> = lines.iter().map(|l| classify(l)).collect();
    // Slots: `%lN = alloca T` / `%fN = alloca i1`.
    let mut slot_ty: HashMap<&str, &str> = HashMap::new();
    for k in &kinds {
        if let Line::Alloca { name, ty } = *k {
            if name.starts_with("%l") || name.starts_with("%f") {
                slot_ty.insert(name, ty);
            }
        }
    }
    if slot_ty.is_empty() {
        let mut out = func.to_string();
        if !out.ends_with('\n') {
            out.push('\n');
        }
        return out;
    }
    // A slot escapes if it's used any other way than as a load/store pointer.
    let mut escaped: HashSet<&str> = HashSet::new();
    for (l, k) in lines.iter().zip(&kinds) {
        let target = match *k {
            Line::Alloca { .. } | Line::Label => continue,
            Line::Load { ptr, .. } | Line::Store { ptr, .. } => Some(ptr),
            Line::Other => None,
        };
        let t = l.trim();
        let toks = names(t);
        for (i, &(a, b)) in toks.iter().enumerate() {
            let n = &t[a..b];
            if slot_ty.contains_key(n) {
                // The pointer of a plain load/store is the line's last token,
                // and it appears nowhere else on the line.
                let last = i + 1 == toks.len() && b == t.len();
                let once = toks.iter().filter(|&&(x, y)| &t[x..y] == n).count() == 1;
                if !(target == Some(n) && last && once) {
                    escaped.insert(n);
                }
            }
        }
    }
    let simple = |n: &str| slot_ty.contains_key(n) && !escaped.contains(n);

    // Forward stores to loads within a block. Renames are resolved when
    // they're recorded, so every value maps straight to its final name.
    let mut rename: HashMap<&str, &str> = HashMap::new();
    let mut drop_line = vec![false; lines.len()];
    let mut known: HashMap<&str, (&str, &str)> = HashMap::new();
    for (i, k) in kinds.iter().enumerate() {
        match *k {
            Line::Label => known.clear(),
            Line::Store { typed, ptr } if simple(ptr) => {
                let ty = slot_ty[ptr];
                match split_typed(typed, ty) {
                    Some(v) => {
                        let v = rename.get(v).copied().unwrap_or(v);
                        known.insert(ptr, (ty, v));
                    }
                    None => {
                        known.remove(ptr);
                    }
                }
            }
            Line::Load { dest, ty, ptr } if simple(ptr) => {
                if let Some(&(sty, v)) = known.get(ptr) {
                    if sty == ty {
                        rename.insert(dest, v);
                        drop_line[i] = true;
                    }
                }
            }
            _ => {}
        }
    }

    // Slots never loaded any more: their stores and allocas go.
    let mut loaded: HashSet<&str> = HashSet::new();
    for (i, k) in kinds.iter().enumerate() {
        if let Line::Load { ptr, .. } = *k {
            if !drop_line[i] {
                loaded.insert(ptr);
            }
        }
    }
    let dead = |p: &str| simple(p) && !loaded.contains(p);
    for (i, k) in kinds.iter().enumerate() {
        match *k {
            Line::Store { ptr, .. } | Line::Alloca { name: ptr, .. } if dead(ptr) => drop_line[i] = true,
            _ => {}
        }
    }

    let mut out = String::with_capacity(func.len());
    for (i, l) in lines.iter().enumerate() {
        if drop_line[i] {
            continue;
        }
        if rename.is_empty() || !l.contains('%') {
            out.push_str(l);
        } else {
            let mut last = 0;
            for (a, b) in names(l) {
                if let Some(v) = rename.get(&l[a..b]) {
                    out.push_str(&l[last..a]);
                    out.push_str(v);
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
