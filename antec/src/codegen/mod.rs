//! Native code generation (M7): MIR → textual LLVM IR, compiled together
//! with the C runtime (`antec/runtime/ante_rt.c`) by `clang`.
//!
//! Generic code is monomorphized: starting from `modhalu`, every reachable
//! body is emitted once per set of concrete types. Trait calls are resolved
//! statically. Values follow these layouts:
//!
//! | "Ante" | LLVM |
//! |---|---|
//! | `ankamN`, `amsam32/64`, `ato_ito`, `aksharam` | `iN`, `float`/`double`, `i1`, `i32` |
//! | `padham`, `varusa[T]` | `{ ptr, i64 len, i64 cap }` (`cap` 0: static) |
//! | `&mukka[T]` | `{ ptr, i64 len }` |
//! | other references, `pette[T]` | `ptr` |
//! | tuples, records | structs |
//! | enums, `undochu`, `sare_thappu` | `{ i32 tag, [k x i64] payload }` |
//! | `pani[…]` | `{ ptr fn, ptr env }` |
//!
//! Until the borrow checker exists (M8), compiled programs never free heap
//! memory: a program that type-checks but misuses a moved value must not be
//! able to corrupt memory. Destructors (`sardhu`) still run on time.

mod ffi;
mod func;
mod glue;
mod map;
mod native;
mod peephole;
mod task;

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;

use crate::mir::{BodyId, Program, Resolved};
use crate::resolve::FileId;
use crate::source::{SourceFile, Span};
use crate::typeck::{AdtKind, FnId, Globals, Typeck};
use crate::types::{Con, Ty};

/// The C runtime, embedded so `antec` is a single binary.
pub const RUNTIME_C: &str =
    concat!(include_str!("../../runtime/ante_rt.c"), include_str!("../../runtime/ante_crypto.c"));

/// Something emitted as one LLVM function.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Item {
    Body(BodyId, Vec<Ty>),
    /// A named function used as a `pani` value: `(env, args…)` → the function.
    Thunk(FnId, Vec<Ty>),
    Show(Ty),
    Eq(Ty),
    Cmp(Ty),
    Xerox(Ty),
    Drop(Ty),
    /// One step of dropping a recursive type off the runtime's drop stack:
    /// its `sardhu`, then its parts pushed (§19.3 order).
    DropStep(Ty),
    /// `i64 hash(ptr v, i64 h)`: mixes a map key into `h` (§21.2 `jodi`).
    Hash(Ty),
    /// `int (*)(const void *, const void *, void *)` for the runtime's sort.
    SortCmp(Ty),
    /// Sort callback that calls a `pani` comparator passed as the context.
    SortBy(Ty),
    /// `undochu[I] next(ptr %iter)` for iterators of item type `I`.
    Next(Ty),
    /// An adapter's `i1 next(ptr state, ptr out)` (§21.4.2).
    Adapter(Adapter, Vec<Ty>),
    /// An adapter's `void drop(ptr state)`.
    AdapterDrop(Adapter, Vec<Ty>),
    /// `void drop(ptr env)` for a closure environment with these captures.
    EnvDrop(Vec<Ty>),
    /// `void run(ptr pani, ptr out)`: a task's body (§24.2).
    TaskRun(Ty),
}

/// The lazy iteration adapters with native support.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Adapter {
    Take,
    Skip,
    Map,
    Filter,
    Zip,
    Enumerate,
    /// A user type implementing `Okkokkati[I]`: calls its `tharvatha`.
    User,
}

pub struct Gen<'a> {
    prog: &'a Program,
    tc: &'a Typeck<'a>,
    g: &'a Globals<'a>,
    files: &'a [SourceFile],
    items: HashMap<Item, String>,
    /// Whether a type contains itself (a list, a tree), by `cyclic`.
    cyclic: HashMap<Ty, bool>,
    queue: VecDeque<Item>,
    /// Named struct types for records and enums, by type.
    named: HashMap<Ty, String>,
    type_defs: String,
    strings: HashMap<String, String>,
    globals: String,
    locs: HashMap<(Option<FileId>, Span), String>,
    funcs: String,
    /// LLVM intrinsic declarations in use.
    intrinsics: std::collections::BTreeSet<String>,
    errors: Vec<String>,
    counter: usize,
    /// C glue for the FFI boundary (§22): struct definitions and functions.
    c_glue: String,
    c_structs: HashMap<Ty, String>,
    /// The glue function of each `bayata "C"` function called.
    ffi: HashMap<FnId, String>,
    /// How a trapped task unwinds its frames, dropping their values (§24.5.2).
    unwind: Unwind,
}

/// How a trapped task's frames are unwound (§24.5.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unwind {
    /// Not at all: a trapped task's values are left as they are.
    Off,
    /// By the C unwinder (`_Unwind_ForcedUnwind`), through landing pads.
    Dwarf,
    /// By Windows' own unwinder, which `longjmp` runs there, through
    /// cleanup funclets.
    Seh,
}

/// A program ready for the C toolchain.
pub struct Emitted {
    pub ir: String,
    /// C source for the FFI boundary, if the program crosses it.
    pub glue: Option<String>,
}

pub fn emit_llvm(
    prog: &Program,
    tc: &Typeck,
    files: &[SourceFile],
    entry: FnId,
    stack: u64,
    task_stack: u64,
    unwind: Unwind,
) -> Result<Emitted, Vec<String>> {
    let mut gen = Gen {
        prog,
        tc,
        g: &tc.globals,
        files,
        items: HashMap::new(),
        cyclic: HashMap::new(),
        queue: VecDeque::new(),
        named: HashMap::new(),
        type_defs: String::new(),
        strings: HashMap::new(),
        globals: String::new(),
        locs: HashMap::new(),
        funcs: String::new(),
        intrinsics: Default::default(),
        errors: Vec::new(),
        counter: 0,
        c_glue: String::new(),
        c_structs: HashMap::new(),
        ffi: HashMap::new(),
        unwind,
    };
    let Some(&body) = prog.fn_bodies.get(&entry) else {
        return Err(vec!["`modhalu` ki body ledhu".into()]);
    };
    let main_fn = gen.item(Item::Body(body, Vec::new()));
    while let Some(item) = gen.queue.pop_front() {
        gen.emit_item(&item);
    }
    gen.emit_main(&main_fn, &tc.globals.fns[entry].ret.clone(), stack, task_stack);
    ffi::exports(&mut gen);
    while let Some(item) = gen.queue.pop_front() {
        gen.emit_item(&item);
    }
    if !gen.errors.is_empty() {
        let mut errs = gen.errors;
        errs.sort();
        errs.dedup();
        return Err(errs);
    }
    let mut out = String::new();
    out.push_str("; \"Ante\" program, generated by antec\n\n");
    out.push_str(
        "%str = type { ptr, i64, i64 }\n%vec = type { ptr, i64, i64 }\n%slice = type { ptr, i64 }\n",
    );
    out.push_str("%pani = type { ptr, ptr }\n%iter = type { i32, i8, i64, i64, ptr, i64 }\n");
    out.push_str("%map = type { ptr, i64, i64, ptr, i64, i64 }\n");
    out.push_str("%loc = type { ptr, i64, i32, i32 }\n");
    out.push_str(&gen.type_defs);
    out.push('\n');
    out.push_str(&gen.globals);
    out.push('\n');
    out.push_str(DECLS);
    for d in &gen.intrinsics {
        out.push_str(d);
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&gen.funcs);
    if gen.unwind != Unwind::Off {
        // Every frame needs unwind tables, so an unwinding trap can pass through it.
        out.push_str("\n!llvm.module.flags = !{!0}\n!0 = !{i32 7, !\"uwtable\", i32 2}\n");
    }
    let glue = (!gen.c_glue.is_empty()).then(|| format!("#include <stdint.h>\n\n{}", gen.c_glue));
    Ok(Emitted { ir: out, glue })
}

/// Whether the program, its runtime or its C glue already uses `name` as a
/// symbol, so `@bayata` can't export under it (§22.4).
pub fn reserved_symbol(name: &str) -> bool {
    name == "main"
        || name.starts_with("ante_ffi_")
        || name.starts_with("ante_exp_")
        || DECLS.contains(&format!("@{name}("))
        || RUNTIME_C.contains(&format!(" {name}("))
}

/// Set to true by `link_native` for release builds (`system._release`).
pub const RELEASE_FLAG: &str = "@ante_release_build = private constant i1 false";

const DECLS: &str = "\
@ante_release_build = private constant i1 false
declare void @ante_start(i32, ptr, ptr, i64) noreturn
declare void @ante_exit(i32) noreturn
declare void @ante_vellipo(i32) noreturn
declare void @ante_flush()
declare void @ante_write(i32, ptr, i32)
declare void @ante_buf_push(ptr, ptr, i64)
declare void @ante_buf_i64(ptr, i64)
declare void @ante_buf_u64(ptr, i64)
declare void @ante_buf_hex(ptr, i64)
declare void @ante_buf_char(ptr, i32)
declare void @ante_buf_f64(ptr, double)
declare void @ante_buf_f32(ptr, float)
declare void @ante_trap(ptr, ptr) noreturn
declare void @ante_trap_overflow(ptr, ptr, i64, i64, i32, i32, ptr) noreturn
declare void @ante_trap_index(i64, i64, ptr) noreturn
declare void @ante_trap_slice(i64, i64, i64, ptr) noreturn
declare void @ante_trap_shift(i64, i32, ptr) noreturn
declare void @ante_trap_code(i32, ptr) noreturn
declare ptr @ante_alloc(i64)
declare ptr @ante_vec_alloc(i64, i64)
declare void @ante_vec_push(ptr, ptr, i64)
declare void @ante_buf_free(ptr)
declare void @ante_str_append(ptr, ptr)
declare void @free(ptr)
declare void @ante_pani_inc(ptr)
declare void @ante_pani_dec(ptr)
declare void @ante_drop_push(ptr, ptr)
declare void @ante_drop_deep(ptr, ptr)
declare void @ante_drop_buf(ptr)
declare i32 @__gcc_personality_v0(...)
declare i32 @__C_specific_handler(...)
declare ptr @ante_team_new()
declare void @ante_team_end(ptr)
declare void @ante_team_cancel(ptr)
declare ptr @ante_task_spawn(ptr, ptr, ptr, i64, ptr)
declare i32 @ante_task_wait(ptr, ptr, i64, ptr, ptr, ptr)
declare void @ante_task_drop(ptr)
declare void @ante_task_cancel(ptr)
declare i32 @ante_task_cancelled()
declare void @ante_task_sleep(i64)
declare i64 @ante_cpus()
declare void @ante_set_task_stack(i64)
declare ptr @ante_lock_new(i64)
declare void @ante_lock_free(ptr)
declare i32 @ante_lock_open(ptr, i32, ptr)
declare void @ante_lock_close(ptr)
declare void @ante_lock_heal(ptr)
declare ptr @ante_atomic_new(i64)
declare void @ante_atomic_free(ptr)
declare i64 @ante_atomic_load(ptr)
declare void @ante_atomic_store(ptr, i64)
declare i64 @ante_atomic_swap(ptr, i64)
declare i64 @ante_atomic_add(ptr, i64, i32, i32)
declare i32 @ante_atomic_cas(ptr, i64, i64, ptr)
declare ptr @ante_chan_new(i64, i64, ptr)
declare i32 @ante_chan_send(ptr, ptr)
declare i32 @ante_chan_recv(ptr, ptr, i32)
declare void @ante_chan_sender_copy(ptr)
declare void @ante_chan_sender_drop(ptr)
declare void @ante_chan_receiver_drop(ptr)
declare ptr @ante_rc_new(i64)
declare void @ante_rc_inc(ptr)
declare i32 @ante_rc_dec(ptr)
declare void @ante_rc_free(ptr)
declare ptr @ante_expect_chan(ptr)
declare ptr @ante_expect_timer(i64)
declare ptr @ante_expect_task(ptr)
declare ptr @ante_expect_cancel()
declare void @ante_expect_free(ptr)
declare i64 @ante_select(ptr, i64, ptr)
declare void @ante_vec_extend(ptr, ptr, i64, i64)
declare void @ante_sort(ptr, i64, i64, ptr, ptr)
declare void @ante_str_concat(ptr, ptr, ptr)
declare void @ante_str_copy(ptr, ptr)
declare i32 @ante_str_cmp(ptr, ptr)
declare i64 @ante_str_chars(ptr)
declare i32 @ante_str_next_char(ptr, ptr, ptr)
declare void @ante_str_trim(ptr, ptr)
declare i32 @ante_str_starts(ptr, ptr)
declare i32 @ante_str_contains(ptr, ptr)
declare void @ante_str_split(ptr, ptr, ptr)
declare i32 @ante_str_parse_int(ptr, i32, i32, ptr)
declare i32 @ante_str_parse_float(ptr, ptr)
declare i32 @ante_file_read(ptr, ptr, ptr)
declare i32 @ante_file_write(ptr, ptr, ptr)
declare i32 @ante_file_exists(ptr)
declare void @ante_args(ptr)
declare i32 @ante_env(ptr, ptr)
declare i32 @ante_read_line(ptr)
declare void @ante_os(ptr)
declare void @ante_arch(ptr)
declare i64 @ante_now_ns()
declare i64 @ante_utc_ns()
declare void @ante_env_text(ptr)
declare i1 @ante_char_in(i32, ptr, i64)
declare i64 @ante_map_find(ptr, i64, ptr, i64, i64, ptr)
declare i64 @ante_map_add(ptr, i64, i64)
declare void @ante_map_remove(ptr, i64, i64)
declare void @ante_map_free(ptr)
declare i64 @ante_hash_text(i64, ptr)
declare ptr @ante_mem_alloc(i64, i64)
declare void @ante_mem_release(ptr, i64, i64)
declare ptr @ante_mem_realloc(ptr, i64, i64, i64)
declare void @llvm.memmove.p0.p0.i64(ptr, ptr, i64, i1)
declare i32 @ante_file_read_bytes(ptr, ptr, ptr)
declare void @ante_bytes_copy(ptr, ptr, i64)
declare void @ante_zero_buf(ptr)
declare void @ante_sha256(ptr, i64, ptr)
declare void @ante_sha512(ptr, i64, ptr)
declare void @ante_blake3(ptr, i64, ptr)
declare i32 @ante_random_bytes(i64, ptr)
declare i32 @ante_ct_eq(ptr, i64, ptr, i64)
declare void @ante_argon2id(ptr, i64, ptr, i64, ptr, i64, ptr, i64, i64, i64, i64, i64, ptr)
declare void @ante_aes_gcm_seal_n(ptr, i64, ptr, i64, ptr, i64, ptr, i64, ptr)
declare i32 @ante_aes_gcm_open_n(ptr, i64, ptr, i64, ptr, i64, ptr, i64, ptr)
declare void @ante_secure_zero(ptr, i64)
declare void @ante_str_from_bytes_lossy(ptr, i64, ptr)
declare i32 @ante_file_write_bytes(ptr, ptr, i64, ptr)
declare i32 @ante_file_is_folder(ptr)
declare i32 @ante_file_list(ptr, ptr, ptr)
declare i32 @ante_file_mkdirs(ptr, ptr)
declare i32 @ante_file_remove(ptr, ptr)
declare void @ante_path_join(ptr, ptr, ptr)
declare void @ante_path_name(ptr, ptr)
declare void @ante_path_folder(ptr, ptr)
declare i32 @ante_path_extension(ptr, ptr)
declare i32 @ante_run(ptr, ptr, ptr, ptr, ptr, ptr)
declare i32 @ante_str_from_bytes(ptr, i64, ptr, ptr)
declare i32 @ante_str_byte_part(ptr, i64, i64, ptr, ptr)
declare double @tan(double)
declare float @tanf(float)
";

impl<'a> Gen<'a> {
    fn intrinsic_decl(&mut self, decl: &str) {
        self.intrinsics.insert(decl.to_string());
    }

    /// The pointer and length of a literal text constant.
    fn str_ptr(&mut self, text: &str) -> (String, usize) {
        let c = self.str_const(text);
        let ptr = c.split_whitespace().nth(2).unwrap_or("null").trim_end_matches(',').to_string();
        (ptr, text.len())
    }

    fn error(&mut self, msg: impl Into<String>) {
        self.errors.push(msg.into());
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}.{}", self.counter)
    }

    /// The LLVM name of an item, queueing it for emission the first time.
    fn item(&mut self, item: Item) -> String {
        if let Some(n) = self.items.get(&item) {
            return n.clone();
        }
        let base = match &item {
            Item::Body(b, _) => format!("ante.{}", sanitize(&self.prog.bodies[*b as usize].name)),
            Item::Thunk(..) => "thunk".into(),
            Item::Show(_) => "show".into(),
            Item::Eq(_) => "eq".into(),
            Item::Cmp(_) => "cmp".into(),
            Item::Xerox(_) => "xerox".into(),
            Item::Drop(_) => "drop".into(),
            Item::DropStep(_) => "dropstep".into(),
            Item::TaskRun(_) => "taskrun".into(),
            Item::Hash(_) => "hash".into(),
            Item::SortCmp(_) => "sortcmp".into(),
            Item::SortBy(_) => "sortby".into(),
            Item::Next(_) => "next".into(),
            Item::Adapter(..) => "adapter".into(),
            Item::AdapterDrop(..) => "adapterdrop".into(),
            Item::EnvDrop(_) => "envdrop".into(),
        };
        let name = format!("@\"{}\"", self.fresh(&base));
        self.items.insert(item.clone(), name.clone());
        self.queue.push_back(item);
        name
    }

    fn emit_item(&mut self, item: &Item) {
        let name = self.items[item].clone();
        let code = match item {
            Item::Body(b, tys) => func::emit_body(self, &name, *b, tys),
            Item::Thunk(fid, tys) => glue::thunk(self, &name, *fid, tys),
            Item::Show(t) => glue::show(self, &name, t),
            Item::Eq(t) => glue::eq(self, &name, t),
            Item::Cmp(t) => glue::cmp(self, &name, t),
            Item::Xerox(t) => glue::xerox(self, &name, t),
            Item::Drop(t) => glue::drop_glue(self, &name, t),
            Item::DropStep(t) => glue::drop_step(self, &name, t),
            Item::Hash(t) => glue::hash(self, &name, t),
            Item::EnvDrop(caps) => glue::env_drop(self, &name, caps),
            Item::TaskRun(t) => task::run_fn(self, &name, t),
            Item::SortCmp(t) => glue::sort_cmp(self, &name, t),
            Item::SortBy(t) => glue::sort_by(self, &name, t),
            Item::Next(t) => native::next_fn(self, &name, t),
            Item::Adapter(a, tys) => native::adapter_fn(self, &name, *a, tys),
            Item::AdapterDrop(a, tys) => native::adapter_drop(self, &name, *a, tys),
        };
        self.funcs.push_str(&peephole::optimize(&code));
        self.funcs.push('\n');
    }

    /// `main` hands `ante.entry` to the runtime, which runs it on a thread
    /// with the `[build] stack` size and turns overflow into T08.
    fn emit_main(&mut self, modhalu: &str, ret: &Ty, stack: u64, task_stack: u64) {
        let rt = self.ll(ret);
        let mut f = String::new();
        let _ = writeln!(f, "define i32 @main(i32 %argc, ptr %argv) {{");
        let _ = writeln!(f, "  call void @ante_set_task_stack(i64 {task_stack})");
        let _ = writeln!(f, "  call void @ante_start(i32 %argc, ptr %argv, ptr @ante.entry, i64 {stack})");
        let _ = writeln!(f, "  unreachable\n}}\n");
        let _ = writeln!(f, "define internal void @ante.entry() {{");
        let _ = writeln!(f, "  %r = call {rt} {modhalu}()");
        if let Ty::App(Con::SareThappu, a) = ret {
            // `thappu(e)`: print it and exit with 1 (§20.2).
            let err_ty = a[1].clone();
            let et = self.ll(&err_ty);
            let pt = self.payload_struct(std::slice::from_ref(&err_ty));
            let show = self.item(Item::Show(err_ty));
            let prefix = self.str_const("thappu: ");
            let _ = writeln!(f, "  %slot = alloca {rt}\n  store {rt} %r, ptr %slot");
            let _ = writeln!(
                f,
                "  %tagp = getelementptr {rt}, ptr %slot, i32 0, i32 0\n  %tag = load i32, ptr %tagp"
            );
            let _ = writeln!(f, "  %bad = icmp eq i32 %tag, 1\n  br i1 %bad, label %fail, label %ok");
            let _ = writeln!(f, "fail:\n  %buf = alloca %str\n  store %str {prefix}, ptr %buf");
            let _ = writeln!(f, "  %pay = getelementptr {rt}, ptr %slot, i32 0, i32 1");
            let _ = writeln!(f, "  %ep = getelementptr {pt}, ptr %pay, i32 0, i32 0");
            let _ = writeln!(
                f,
                "  call void {show}(ptr %buf, ptr %ep)\n  call void @ante_write(i32 2, ptr %buf, i32 1)"
            );
            let _ = writeln!(f, "  call void @ante_exit(i32 1)\n  unreachable\nok:");
            let _ = et;
        }
        let _ = writeln!(f, "  call void @ante_exit(i32 0)\n  unreachable\n}}");
        self.funcs.push_str(&f);
    }

    // ── constants ──────────────────────────────────────────────────────────

    /// A `%str` constant for literal text.
    pub(super) fn str_const(&mut self, text: &str) -> String {
        let name = match self.strings.get(text) {
            Some(n) => n.clone(),
            None => {
                let n = format!("@\"{}\"", self.fresh("s"));
                let _ = writeln!(
                    self.globals,
                    "{n} = private unnamed_addr constant [{} x i8] c\"{}\"",
                    text.len(),
                    escape(text)
                );
                self.strings.insert(text.to_string(), n.clone());
                n
            }
        };
        format!("{{ ptr {name}, i64 {}, i64 0 }}", text.len())
    }

    /// A sorted table of inclusive code-point ranges `[lo, hi, lo, hi, …]`
    /// for a Unicode property, built from Rust's own tables so compiled
    /// programs agree with the interpreter. Returns the global and the
    /// number of ranges.
    fn char_table(&mut self, kind: &str, pred: fn(char) -> bool) -> (String, usize) {
        let key = format!("\0t{kind}");
        if let Some(entry) = self.strings.get(&key) {
            let (name, n) = entry.split_once(' ').unwrap_or((entry, "0"));
            return (name.to_string(), n.parse().unwrap_or(0));
        }
        let mut ranges: Vec<(u32, u32)> = Vec::new();
        for c in (0..=0x10FFFFu32).filter_map(char::from_u32) {
            if !pred(c) {
                continue;
            }
            let c = c as u32;
            match ranges.last_mut() {
                Some((_, hi)) if *hi + 1 == c => *hi = c,
                _ => ranges.push((c, c)),
            }
        }
        let name = format!("@\"{}\"", self.fresh("ctab"));
        let items: Vec<String> =
            ranges.iter().flat_map(|(a, b)| [format!("i32 {a}"), format!("i32 {b}")]).collect();
        let _ = writeln!(
            self.globals,
            "{name} = private unnamed_addr constant [{} x i32] [{}]",
            items.len(),
            items.join(", ")
        );
        self.strings.insert(key, format!("{name} {}", ranges.len()));
        (name, ranges.len())
    }

    /// A C string constant (NUL-terminated), as a pointer.
    fn cstr(&mut self, text: &str) -> String {
        let key = format!("\0c{text}");
        if let Some(n) = self.strings.get(&key) {
            return n.clone();
        }
        let n = format!("@\"{}\"", self.fresh("c"));
        let _ = writeln!(
            self.globals,
            "{n} = private unnamed_addr constant [{} x i8] c\"{}\\00\"",
            text.len() + 1,
            escape(text)
        );
        self.strings.insert(key, n.clone());
        n
    }

    /// A source location for trap messages (§20.3).
    fn loc(&mut self, file: Option<FileId>, span: Span) -> String {
        if let Some(n) = self.locs.get(&(file, span)) {
            return n.clone();
        }
        let (name, line, col) = match file.and_then(|f| self.files.get(f)) {
            Some(src) => {
                let (l, c) = src.line_col(span.start);
                (src.name.clone(), l, c)
            }
            None => ("?".to_string(), 0, 0),
        };
        let fname = self.str_const(&name);
        let fptr = fname.split_whitespace().nth(2).unwrap_or("null").trim_end_matches(',').to_string();
        let n = format!("@\"{}\"", self.fresh("loc"));
        let _ = writeln!(
            self.globals,
            "{n} = private unnamed_addr constant %loc {{ ptr {fptr}, i64 {}, i32 {line}, i32 {col} }}",
            name.len()
        );
        self.locs.insert((file, span), n.clone());
        n
    }

    // ── layouts ────────────────────────────────────────────────────────────

    /// The LLVM type of a value of `ty`.
    fn ll(&mut self, ty: &Ty) -> String {
        match ty {
            Ty::Int(i) => format!("i{}", i.bits),
            Ty::Float(32) => "float".into(),
            Ty::Float(_) => "double".into(),
            Ty::Bool => "i1".into(),
            Ty::Char => "i32".into(),
            Ty::Unit | Ty::Never => "{}".into(),
            Ty::Str => "%str".into(),
            // A long tuple gets a name, so the IR doesn't spell it out at
            // every use (quadratic in its length).
            Ty::Tuple(items) if items.len() > 8 => {
                if let Some(n) = self.named.get(ty) {
                    return n.clone();
                }
                let name = format!("%\"{}\"", self.fresh("tuple"));
                self.named.insert(ty.clone(), name.clone());
                let body = self.struct_body(items);
                let _ = writeln!(self.type_defs, "{name} = type {body}");
                name
            }
            Ty::Tuple(items) => self.struct_body(items),
            Ty::Array(e, Some(n)) => format!("[{n} x {}]", self.ll(e)),
            Ty::Ref(_, inner) | Ty::Ptr(_, inner) => {
                if matches!(**inner, Ty::App(Con::Mukka, _)) {
                    "%slice".into()
                } else {
                    "ptr".into()
                }
            }
            Ty::App(Con::Pette | Con::Panchukunna, _) => "ptr".into(),
            Ty::App(Con::Varusa, _) => "%vec".into(),
            Ty::App(Con::Mukka, a) => {
                // Only places have this type; values are behind `&`.
                format!("[0 x {}]", self.ll(&a[0]))
            }
            Ty::App(Con::Paridhi, a) => {
                let i = self.ll(&a[0]);
                format!("{{ {i}, {i}, i1 }}")
            }
            Ty::App(Con::Pravaham, _) => "%iter".into(),
            Ty::App(Con::Jodi | Con::GajibijiJodi | Con::Gumpu, _) => "%map".into(),
            Ty::Fn(..) => "%pani".into(),
            Ty::App(Con::Undochu | Con::SareThappu | Con::Adt(_), _) => self.named_type(ty),
            // A type inference left open: the value is never looked at.
            Ty::Var(_) => "{}".into(),
            other => {
                let shown = crate::typeck::show_ty(self.g, other);
                self.error(format!("native backend lo ee type inka ledhu: {shown}"));
                "{}".into()
            }
        }
    }

    fn is_enum(&self, ty: &Ty) -> bool {
        match ty {
            Ty::App(Con::Undochu | Con::SareThappu, _) => true,
            Ty::App(Con::Adt(d), _) => matches!(self.g.adts.get(d).map(|a| &a.kind), Some(AdtKind::Enum(_))),
            _ => false,
        }
    }

    /// The field types of each variant of an enum (or of a record's one "variant").
    fn variants(&self, ty: &Ty) -> Vec<Vec<Ty>> {
        match ty {
            Ty::App(Con::Undochu, a) => vec![vec![a[0].clone()], vec![]],
            Ty::App(Con::SareThappu, a) => vec![vec![a[0].clone()], vec![a[1].clone()]],
            Ty::App(Con::Adt(d), args) => match self.g.adts.get(d).map(|a| &a.kind) {
                Some(AdtKind::Enum(vs)) => {
                    vs.iter().map(|v| v.fields.iter().map(|f| f.subst(args)).collect()).collect()
                }
                Some(AdtKind::Record(fs)) => vec![fs.iter().map(|f| f.ty.subst(args)).collect()],
                None => vec![vec![]],
            },
            _ => vec![vec![]],
        }
    }

    /// Whether `ty` contains itself, through boxes, collections, options or
    /// fields: dropping such a value may go arbitrarily deep, so it's done
    /// off the runtime's drop stack instead of the C stack.
    fn cyclic(&mut self, ty: &Ty) -> bool {
        if let Some(&c) = self.cyclic.get(ty) {
            return c;
        }
        let mut seen: HashSet<Ty> = HashSet::new();
        let mut work = self.parts(ty);
        let mut found = false;
        while let Some(t) = work.pop() {
            if t == *ty {
                found = true;
                break;
            }
            // Generic records can grow their types forever; that far in,
            // just assume so.
            if seen.len() > 4096 {
                found = true;
                break;
            }
            if seen.insert(t.clone()) {
                work.extend(self.parts(&t));
            }
        }
        self.cyclic.insert(ty.clone(), found);
        found
    }

    /// The types a value of `ty` directly owns, as far as dropping goes.
    fn parts(&self, ty: &Ty) -> Vec<Ty> {
        match ty {
            Ty::Tuple(items) => items.clone(),
            Ty::Array(e, _) => vec![(**e).clone()],
            Ty::App(Con::Pette | Con::Varusa, a) => a.clone(),
            Ty::App(Con::Undochu | Con::SareThappu | Con::Adt(_), _) => self.variants(ty).concat(),
            _ => Vec::new(),
        }
    }

    fn named_type(&mut self, ty: &Ty) -> String {
        if let Some(n) = self.named.get(ty) {
            return n.clone();
        }
        let base = match ty {
            Ty::App(Con::Undochu, _) => "undochu".to_string(),
            Ty::App(Con::SareThappu, _) => "sare_thappu".to_string(),
            Ty::App(Con::Adt(d), _) => sanitize(&self.g.res.defs[*d].name),
            _ => "t".into(),
        };
        let name = format!("%\"{}\"", self.fresh(&base));
        self.named.insert(ty.clone(), name.clone());
        let body = if self.is_enum(ty) {
            let mut max = 0u64;
            for fields in self.variants(ty) {
                let (size, _) = self.size_align(&Ty::Tuple(fields));
                max = max.max(size);
            }
            format!("{{ i32, [{} x i64] }}", max.div_ceil(8))
        } else {
            let fields = self.variants(ty).pop().unwrap_or_default();
            self.struct_body(&fields)
        };
        let _ = writeln!(self.type_defs, "{name} = type {body}");
        name
    }

    /// `{ a, b, … }`, spelled out.
    fn struct_body(&mut self, items: &[Ty]) -> String {
        let parts: Vec<String> = items.iter().map(|t| self.ll(t)).collect();
        format!("{{ {} }}", parts.join(", "))
    }

    /// The anonymous struct type of a variant's payload.
    fn payload_struct(&mut self, fields: &[Ty]) -> String {
        self.ll(&Ty::Tuple(fields.to_vec()))
    }

    /// Size and alignment, matching LLVM's layout on 64-bit targets.
    fn size_align(&mut self, ty: &Ty) -> (u64, u64) {
        let structure = |items: Vec<(u64, u64)>| {
            let (mut size, mut align) = (0u64, 1u64);
            for (s, a) in items {
                size = size.div_ceil(a) * a + s;
                align = align.max(a);
            }
            (size.div_ceil(align) * align, align)
        };
        match ty {
            Ty::Int(i) => {
                let b = u64::from(i.bits / 8).max(1);
                (b, b)
            }
            Ty::Float(32) | Ty::Char => (4, 4),
            Ty::Float(_) => (8, 8),
            Ty::Bool => (1, 1),
            Ty::Unit | Ty::Never | Ty::Var(_) => (0, 1),
            Ty::Str | Ty::App(Con::Varusa, _) => (24, 8),
            Ty::Fn(..) => (16, 8),
            Ty::Ref(_, inner) | Ty::Ptr(_, inner) => {
                if matches!(**inner, Ty::App(Con::Mukka, _)) {
                    (16, 8)
                } else {
                    (8, 8)
                }
            }
            Ty::App(Con::Pette | Con::Panchukunna, _) => (8, 8),
            Ty::App(Con::Pravaham, _) => (40, 8),
            Ty::App(Con::Jodi | Con::GajibijiJodi | Con::Gumpu, _) => (48, 8),
            Ty::App(Con::Paridhi, a) => {
                let e = self.size_align(&a[0]);
                structure(vec![e, e, (1, 1)])
            }
            Ty::Tuple(items) => {
                let parts = items.iter().map(|t| self.size_align(t)).collect();
                structure(parts)
            }
            Ty::Array(e, n) => {
                let (s, a) = self.size_align(e);
                (s * n.unwrap_or(0), a)
            }
            Ty::App(Con::Undochu | Con::SareThappu | Con::Adt(_), _) => {
                if self.is_enum(ty) {
                    let mut max = 0u64;
                    for fields in self.variants(ty) {
                        max = max.max(self.size_align(&Ty::Tuple(fields)).0);
                    }
                    structure(vec![(4, 4), (max.div_ceil(8) * 8, 8)])
                } else {
                    let fields = self.variants(ty).pop().unwrap_or_default();
                    self.size_align(&Ty::Tuple(fields))
                }
            }
            _ => (8, 8),
        }
    }

    fn resolve(&self, fid: FnId, tys: &[Ty]) -> Option<Resolved> {
        crate::mir::resolve_trait(self.prog, self.g, fid, tys)
    }
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect()
}

/// Escapes text for an LLVM `c"…"` string.
fn escape(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if (0x20..0x7f).contains(&b) && b != b'"' && b != b'\\' {
            out.push(b as char);
        } else {
            let _ = write!(out, "\\{b:02X}");
        }
    }
    out
}

/// An LLVM float literal (the exact bits, as LLVM requires).
pub fn float_lit(bits: u8, v: f64) -> String {
    if bits == 32 {
        // Float constants are written as the double with the same value.
        format!("0x{:016X}", f64::from(v as f32).to_bits())
    } else {
        format!("0x{:016X}", v.to_bits())
    }
}
