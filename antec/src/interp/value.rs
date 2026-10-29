//! Runtime values of the interpreter.

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use crate::mir::BodyId;
use crate::typeck::FnId;
use crate::types::{IntTy, Ty};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Root {
    Stack(usize),
    Heap(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elem {
    /// A field of a record, tuple or variant.
    Field(u32),
    /// An element of an array or `varusa`.
    Index(usize),
    /// The contents of a `pette`.
    Box,
    /// The key or value of a map entry.
    MapKey(usize),
    MapVal(usize),
}

/// Where a value lives: a root slot and a path into it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Addr {
    pub root: Root,
    pub path: Vec<Elem>,
}

impl Addr {
    pub fn with(&self, e: Elem) -> Addr {
        let mut a = self.clone();
        a.path.push(e);
        a
    }
}

#[derive(Debug)]
pub struct Closure {
    pub body: BodyId,
    pub captures: Vec<Value>,
    pub tys: Rc<[Ty]>,
}

#[derive(Clone, Debug)]
pub enum Value {
    /// Moved out, or not yet written.
    Uninit,
    Unit,
    Bool(bool),
    /// Every integer type; the operation knows the width.
    Int(i128),
    F32(f32),
    F64(f64),
    Char(char),
    Str(Rc<str>),
    /// The growing text inside a `Raatha`.
    Buf(String),
    Tuple(Vec<Value>),
    Array(Vec<Value>),
    Vec(Vec<Value>),
    /// Records (variant 0), enums, `undochu` and `sare_thappu`.
    Adt(u32, Vec<Value>),
    Map(Box<OrdMap>),
    Box(Box<Value>),
    Ref(Addr),
    /// `&mukka[T]`: a sequence, a start and a length.
    Slice(Addr, usize, usize),
    /// `Paridhi`: low, high, inclusive, element type.
    Range(i128, i128, bool, IntTy),
    Fn(FnId, Rc<[Ty]>),
    Closure(Rc<Closure>),
    Iter(Box<IterState>),
}

impl Value {
    pub fn elem(&self, e: &Elem) -> &Value {
        match (self, e) {
            (Value::Tuple(v) | Value::Adt(_, v), Elem::Field(i)) => &v[*i as usize],
            (Value::Array(v) | Value::Vec(v), Elem::Index(i)) => &v[*i],
            (Value::Array(v) | Value::Vec(v), Elem::Field(i)) => &v[*i as usize],
            (Value::Box(b), Elem::Box) => b,
            (Value::Map(m), Elem::MapKey(i)) => m.key(*i),
            (Value::Map(m), Elem::MapVal(i)) => m.val(*i),
            _ => &Value::Uninit,
        }
    }

    pub fn elem_mut(&mut self, e: &Elem) -> &mut Value {
        match (self, e) {
            (Value::Tuple(v) | Value::Adt(_, v), Elem::Field(i)) => &mut v[*i as usize],
            (Value::Array(v) | Value::Vec(v), Elem::Index(i)) => &mut v[*i],
            (Value::Array(v) | Value::Vec(v), Elem::Field(i)) => &mut v[*i as usize],
            (Value::Box(b), Elem::Box) => b,
            (Value::Map(m), Elem::MapVal(i)) => m.val_mut(*i),
            (Value::Map(m), Elem::MapKey(i)) => m.key_mut(*i),
            (v, _) => v,
        }
    }

    /// The length of an array or `varusa`.
    pub fn seq_len(&self) -> Option<usize> {
        match self {
            Value::Array(v) | Value::Vec(v) => Some(v.len()),
            _ => None,
        }
    }

    pub fn str(&self) -> Rc<str> {
        match self {
            Value::Str(s) => s.clone(),
            Value::Buf(s) => Rc::from(s.as_str()),
            _ => Rc::from(""),
        }
    }

    pub fn int(&self) -> i128 {
        match self {
            Value::Int(i) => *i,
            Value::Char(c) => *c as i128,
            Value::Bool(b) => *b as i128,
            _ => 0,
        }
    }

    pub fn some(v: Value) -> Value {
        Value::Adt(0, vec![v])
    }

    pub fn none() -> Value {
        Value::Adt(1, Vec::new())
    }

    pub fn ok(v: Value) -> Value {
        Value::Adt(0, vec![v])
    }

    pub fn err(v: Value) -> Value {
        Value::Adt(1, vec![v])
    }
}

/// A hashable image of a key value (§21.2 `jodi`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Unit,
    Bool(bool),
    Int(i128),
    Char(char),
    Str(Rc<str>),
    Float(u64),
    Seq(Vec<Key>),
    Adt(u32, Vec<Key>),
    Other,
}

pub fn key_of(v: &Value) -> Key {
    match v {
        Value::Unit => Key::Unit,
        Value::Bool(b) => Key::Bool(*b),
        Value::Int(i) => Key::Int(*i),
        Value::Char(c) => Key::Char(*c),
        Value::Str(_) | Value::Buf(_) => Key::Str(v.str()),
        Value::F32(x) => Key::Float(f64::from(*x).to_bits()),
        Value::F64(x) => Key::Float(x.to_bits()),
        Value::Tuple(xs) | Value::Array(xs) | Value::Vec(xs) => Key::Seq(xs.iter().map(key_of).collect()),
        Value::Adt(i, xs) => Key::Adt(*i, xs.iter().map(key_of).collect()),
        Value::Box(b) => key_of(b),
        _ => Key::Other,
    }
}

/// An insertion-ordered map; removed entries leave a hole so positions
/// (and references into entries) stay put. Entries are indexed by the key's
/// hash (`Machine::key_hash`); the interpreter compares candidates with the
/// key type's `==`, so a type's own `Polchochu` and `HashCheyochu` decide
/// which keys are the same (§21.2).
#[derive(Clone, Debug, Default)]
pub struct OrdMap {
    entries: Vec<Option<(Value, Value)>>,
    index: HashMap<u64, Vec<usize>>,
    live: usize,
}

impl OrdMap {
    pub fn len(&self) -> usize {
        self.live
    }

    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// Live entry positions whose key has hash `h`.
    pub fn with_hash(&self, h: u64) -> Vec<usize> {
        self.index.get(&h).cloned().unwrap_or_default()
    }

    /// Adds an entry for a key known to be missing; returns its position.
    pub fn push(&mut self, h: u64, k: Value, v: Value) -> usize {
        let i = self.entries.len();
        self.entries.push(Some((k, v)));
        self.index.entry(h).or_default().push(i);
        self.live += 1;
        i
    }

    /// Takes out entry `i`, whose key has hash `h`.
    pub fn remove_at(&mut self, h: u64, i: usize) -> Option<(Value, Value)> {
        if let Some(bucket) = self.index.get_mut(&h) {
            bucket.retain(|&j| j != i);
            if bucket.is_empty() {
                self.index.remove(&h);
            }
        }
        let e = self.entries.get_mut(i)?.take();
        if e.is_some() {
            self.live -= 1;
        }
        e
    }

    /// Live entry positions in insertion order.
    pub fn positions(&self) -> Vec<usize> {
        (0..self.entries.len()).filter(|&i| self.entries[i].is_some()).collect()
    }

    pub fn key(&self, i: usize) -> &Value {
        self.entries.get(i).and_then(|e| e.as_ref()).map_or(&Value::Uninit, |(k, _)| k)
    }

    pub fn val(&self, i: usize) -> &Value {
        self.entries.get(i).and_then(|e| e.as_ref()).map_or(&Value::Uninit, |(_, v)| v)
    }

    pub fn key_mut(&mut self, i: usize) -> &mut Value {
        &mut self.entries[i].as_mut().expect("live entry").0
    }

    pub fn val_mut(&mut self, i: usize) -> &mut Value {
        &mut self.entries[i].as_mut().expect("live entry").1
    }

    pub fn into_entries(self) -> Vec<(Value, Value)> {
        self.entries.into_iter().flatten().collect()
    }
}

/// A lazy iterator (§21.4).
#[derive(Clone, Debug)]
pub enum IterState {
    Range {
        next: i128,
        last: i128,
        done: bool,
    },
    /// Over a borrowed sequence: base address, start, length, position.
    Seq {
        base: Addr,
        start: usize,
        len: usize,
        pos: usize,
        copy: bool,
    },
    /// Owned items still to come, of this type (dropped with the iterator).
    Owned(VecDeque<Value>, Ty),
    Chars {
        text: Rc<str>,
        pos: usize,
    },
    /// Over a borrowed map: entry positions still to visit.
    Map {
        base: Addr,
        positions: VecDeque<usize>,
        keys_only: bool,
    },
    /// The first side's item type, to drop an item left without a partner.
    Zip(Box<IterState>, Box<IterState>, Ty),
    Take(Box<IterState>, usize),
    /// The item type, to drop skipped items.
    Skip(Box<IterState>, usize, Ty),
    Map2(Box<IterState>, Value),
    /// The item type, to drop rejected items.
    Filter(Box<IterState>, Value, Ty),
    Enumerate(Box<IterState>, i128),
    /// A user type implementing `Okkokkati`: the value and its `tharvatha`.
    User {
        value: Box<Value>,
        next: FnId,
        tys: Vec<Ty>,
    },
}
