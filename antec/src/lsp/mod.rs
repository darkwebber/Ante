//! `antec lsp`: the "Ante" language server. Speaks LSP (JSON-RPC with
//! `Content-Length` framing) over stdin and stdout.
//!
//! Every open file is re-checked (with its package, when it sits in one's
//! `src/`) on open and save, and shortly after the typing stops. Each check
//! publishes diagnostics and builds an `Index` that hover, go to
//! definition, references, rename and completion answer from.

mod index;
pub mod json;
mod pos;

use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::diag::Severity;
use crate::source::{SourceFile, Span};
use index::{Index, Sym, SymKind};
use json::{obj, Json};
use pos::Lines;

/// How long typing must pause before a re-check.
const DEBOUNCE: Duration = Duration::from_millis(200);

/// Runs the server until `exit` or the end of input. Returns the process
/// exit code: 0 after a proper `shutdown`, 1 otherwise.
pub fn run(input: impl Read + Send + 'static, output: impl Write) -> i32 {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("antec-lsp-reader".into())
        .spawn(move || {
            let mut input = io::BufReader::new(input);
            loop {
                let msg = read_message(&mut input);
                let end = !matches!(msg, Ok(Some(_)));
                if tx.send(msg).is_err() || end {
                    return;
                }
            }
        })
        .expect("antec lsp reader thread start avvaledhu");
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        eprintln!("antec lsp: lopala bug 🐛 (ee request skip chesthunnam): {info}");
    }));
    let mut server = Server::new(output);
    let code = server.serve(&rx);
    std::panic::set_hook(previous);
    code
}

/// Reads one framed message; `Ok(None)` at the end of input.
fn read_message(input: &mut impl BufRead) -> io::Result<Option<Json>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            if length.is_some() {
                break;
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                length = value.trim().parse::<usize>().ok();
            }
        }
    }
    let length = length.unwrap_or(0);
    if length > 256 << 20 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too big"));
    }
    let mut body = vec![0; length];
    input.read_exact(&mut body)?;
    let text = String::from_utf8_lossy(&body);
    Json::parse(&text).map(Some).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

struct Doc {
    uri: String,
    /// The path checked for this document (canonical when it exists).
    path: PathBuf,
    text: String,
}

/// What one check covers: a package, or a lone file.
struct Unit {
    index: Option<Index>,
    /// The path of each file in `index`, by file id.
    paths: Vec<Option<PathBuf>>,
    /// URIs that have diagnostics showing.
    published: HashSet<String>,
}

struct Server<W: Write> {
    out: W,
    docs: HashMap<String, Doc>,
    units: HashMap<PathBuf, Unit>,
    /// Units waiting for a re-check, and when it's due.
    due: HashMap<PathBuf, Instant>,
    shutdown: bool,
}

/// A request that failed: `(code, message)`.
type Failure = (i64, String);

const INVALID_PARAMS: i64 = -32602;
const INTERNAL: i64 = -32603;
const REQUEST_FAILED: i64 = -32803;

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

impl<W: Write> Server<W> {
    fn new(out: W) -> Self {
        Server { out, docs: HashMap::new(), units: HashMap::new(), due: HashMap::new(), shutdown: false }
    }

    fn serve(&mut self, rx: &mpsc::Receiver<io::Result<Option<Json>>>) -> i32 {
        loop {
            let wait = self.due.values().min().map(|t| t.saturating_duration_since(Instant::now()));
            let msg = match wait {
                Some(w) => match rx.recv_timeout(w) {
                    Ok(m) => m,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        self.check_due();
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return 1,
                },
                None => match rx.recv() {
                    Ok(m) => m,
                    Err(_) => return 1,
                },
            };
            let msg = match msg {
                Ok(Some(m)) => m,
                Ok(None) => return 1,
                Err(e) => {
                    eprintln!("antec lsp: message chadhavaledhu: {e}");
                    return 1;
                }
            };
            if msg.get("method").str() == Some("exit") {
                return if self.shutdown { 0 } else { 1 };
            }
            self.dispatch(&msg);
        }
    }

    fn send(&mut self, msg: Json) {
        let body = msg.to_string();
        let _ = write!(self.out, "Content-Length: {}\r\n\r\n{body}", body.len());
        let _ = self.out.flush();
    }

    fn notify(&mut self, method: &str, params: Json) {
        self.send(obj([("jsonrpc", "2.0".into()), ("method", method.into()), ("params", params)]));
    }

    fn dispatch(&mut self, msg: &Json) {
        let method = msg.get("method").str().unwrap_or("").to_string();
        let id = msg.get("id").clone();
        let params = msg.get("params");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.handle(&method, params)));
        if id.is_null() {
            return;
        }
        let reply = match result {
            Ok(Ok(value)) => obj([("jsonrpc", "2.0".into()), ("id", id), ("result", value)]),
            Ok(Err((code, message))) => error_reply(id, code, message),
            Err(_) => error_reply(id, INTERNAL, "antec lo bug 🐛, ee request fail ayyindhi".into()),
        };
        self.send(reply);
    }

    fn handle(&mut self, method: &str, p: &Json) -> Result<Json, Failure> {
        match method {
            "initialize" => Ok(capabilities()),
            "initialized" | "$/cancelRequest" | "$/setTrace" | "workspace/didChangeConfiguration" => {
                Ok(Json::Null)
            }
            "shutdown" => {
                self.shutdown = true;
                Ok(Json::Null)
            }
            "textDocument/didOpen" => {
                let doc = p.get("textDocument");
                let uri = doc.get("uri").str().unwrap_or("").to_string();
                let text = doc.get("text").str().unwrap_or("").to_string();
                self.open(uri, text);
                Ok(Json::Null)
            }
            "textDocument/didChange" => {
                let uri = p.get("textDocument").get("uri").str().unwrap_or("");
                // Full sync: the last change holds the whole text.
                if let Some(text) = p.get("contentChanges").arr().last().and_then(|c| c.get("text").str()) {
                    if let Some(doc) = self.docs.get_mut(uri) {
                        doc.text = text.to_string();
                        let key = unit_key(&doc.path);
                        self.due.insert(key, Instant::now() + DEBOUNCE);
                    }
                }
                Ok(Json::Null)
            }
            "textDocument/didSave" => {
                if let Some(doc) = self.docs.get(p.get("textDocument").get("uri").str().unwrap_or("")) {
                    let key = unit_key(&doc.path);
                    self.due.remove(&key);
                    self.check(&key);
                }
                Ok(Json::Null)
            }
            "textDocument/didClose" => {
                let uri = p.get("textDocument").get("uri").str().unwrap_or("");
                if let Some(doc) = self.docs.remove(uri) {
                    let key = unit_key(&doc.path);
                    let still_open = self.docs.values().any(|d| unit_key(&d.path) == key);
                    if still_open {
                        self.check(&key);
                    } else {
                        self.due.remove(&key);
                        if let Some(unit) = self.units.remove(&key) {
                            for uri in unit.published {
                                self.publish(&uri, Vec::new());
                            }
                        }
                    }
                }
                Ok(Json::Null)
            }
            "textDocument/hover" => self.hover(p),
            "textDocument/definition" => self.definition(p),
            "textDocument/references" => self.references(p),
            "textDocument/prepareRename" => self.prepare_rename(p),
            "textDocument/rename" => self.rename(p),
            "textDocument/completion" => self.completion(p),
            "textDocument/formatting" => self.formatting(p),
            m if m.starts_with("$/") => Ok(Json::Null),
            m => Err((-32601, format!("`{m}` support ledhu"))),
        }
    }

    // ── documents and checks ───────────────────────────────────────────────

    fn open(&mut self, uri: String, text: String) {
        let path = match uri_to_path(&uri) {
            Some(p) => canonical(&p),
            // An unsaved buffer: a path that can't be in any package.
            None => PathBuf::from(format!("/<{}>/untitled.ante", fnv(&uri))),
        };
        let key = unit_key(&path);
        self.docs.insert(uri.clone(), Doc { uri, path, text });
        self.due.remove(&key);
        self.check(&key);
    }

    /// Re-checks every unit whose pause is over.
    fn check_due(&mut self) {
        let now = Instant::now();
        let ready: Vec<PathBuf> =
            self.due.iter().filter(|(_, &t)| t <= now).map(|(k, _)| k.clone()).collect();
        for key in ready {
            self.due.remove(&key);
            self.check(&key);
        }
    }

    /// Makes sure the unit of `uri` reflects the latest text.
    fn flush(&mut self, uri: &str) {
        if let Some(doc) = self.docs.get(uri) {
            let key = unit_key(&doc.path);
            if self.due.remove(&key).is_some() || !self.units.contains_key(&key) {
                self.check(&key);
            }
        }
    }

    fn check(&mut self, key: &Path) {
        let overlay: HashMap<PathBuf, String> =
            self.docs.values().map(|d| (d.path.clone(), d.text.clone())).collect();
        let read = |p: &Path| -> io::Result<Vec<u8>> {
            match overlay.get(p).or_else(|| overlay.get(&canonical(p))) {
                Some(text) => Ok(text.clone().into_bytes()),
                None => std::fs::read(p),
            }
        };
        let package = key.join("ante.toml").is_file();
        let target = if package { key.join("src") } else { key.to_path_buf() };
        let mut index = None;
        let checked = crate::package::check_for_editor(&target, &read, &mut |i| {
            let parsed = i.modules.iter().all(|m| m.ast.is_some());
            if parsed {
                index = Some(Index::build(&i));
            }
        });
        let checked = match checked {
            Ok(c) => c,
            Err(e) => {
                eprintln!("antec lsp: `{}` check cheyyalekapoyam: {e}", target.display());
                return;
            }
        };
        let paths: Vec<Option<PathBuf>> = checked
            .sources
            .iter()
            .map(|s| {
                if s.name == crate::stdlib::ANTE_SOURCE_FILE {
                    None
                } else if package {
                    Some(key.join(&s.name))
                } else {
                    Some(key.to_path_buf())
                }
            })
            .collect();
        let mut by_uri: HashMap<String, Vec<Json>> = HashMap::new();
        for path in paths.iter().flatten() {
            by_uri.entry(self.uri_of(path)).or_default();
        }
        for (file, d) in &checked.diagnostics {
            let Some(Some(path)) = paths.get(*file) else { continue };
            let src = &checked.sources[*file];
            let lines = Lines::new(&src.text);
            let severity: usize = if d.severity == Severity::Error { 1 } else { 2 };
            let diag = obj([
                ("range", lines.range(d.span)),
                ("severity", severity.into()),
                ("code", d.code.into()),
                ("source", "antec".into()),
                ("message", d.message.clone().into()),
            ]);
            by_uri.entry(self.uri_of(path)).or_default().push(diag);
        }
        let old = self.units.remove(key);
        let (old_index, old_paths, mut old_published) = match old {
            Some(u) => (u.index, u.paths, u.published),
            None => (None, Vec::new(), HashSet::new()),
        };
        for (uri, diags) in &by_uri {
            old_published.remove(uri);
            self.publish(uri, diags.clone());
        }
        for uri in old_published {
            self.publish(&uri, Vec::new());
        }
        // A check that couldn't parse keeps the last index, for completion.
        let (index, paths) = match index {
            Some(ix) => (Some(ix), paths),
            None => (old_index, old_paths),
        };
        let unit = Unit { index, paths, published: by_uri.into_keys().collect() };
        self.units.insert(key.to_path_buf(), unit);
    }

    fn publish(&mut self, uri: &str, diagnostics: Vec<Json>) {
        self.notify(
            "textDocument/publishDiagnostics",
            obj([("uri", uri.into()), ("diagnostics", Json::Arr(diagnostics))]),
        );
    }

    /// The URI the client uses for `path`.
    fn uri_of(&self, path: &Path) -> String {
        match self.docs.values().find(|d| d.path == path) {
            Some(d) => d.uri.clone(),
            None => path_to_uri(path),
        }
    }

    // ── positions ──────────────────────────────────────────────────────────

    /// The unit, file and byte offset (in the indexed text) of a request's
    /// `textDocument` and `position`. `fresh` demands the index matches
    /// the text being edited.
    fn locate(&mut self, p: &Json, fresh: bool) -> Result<Option<(PathBuf, usize, usize)>, Failure> {
        let uri = p.get("textDocument").get("uri").str().ok_or((INVALID_PARAMS, "uri ledhu".to_string()))?;
        self.flush(uri);
        let Some(doc) = self.docs.get(uri) else { return Ok(None) };
        let key = unit_key(&doc.path);
        let Some(unit) = self.units.get(&key) else { return Ok(None) };
        let Some(index) = &unit.index else { return Ok(None) };
        let Some(file) = unit.paths.iter().position(|q| q.as_deref() == Some(doc.path.as_path())) else {
            return Ok(None);
        };
        let text = &index.files[file].text;
        if fresh && *text != doc.text {
            return Ok(None);
        }
        let line = p.get("position").get("line").num().unwrap_or(0.0) as usize;
        let col = p.get("position").get("character").num().unwrap_or(0.0) as usize;
        let at = Lines::new(text).offset(line, col);
        Ok(Some((key, file, at)))
    }

    fn location(&self, unit: &Unit, file: usize, span: Span) -> Option<Json> {
        let path = unit.paths.get(file)?.as_ref()?;
        let index = unit.index.as_ref()?;
        let lines = Lines::new(&index.files[file].text);
        Some(obj([("uri", self.uri_of(path).into()), ("range", lines.range(span))]))
    }

    // ── features ───────────────────────────────────────────────────────────

    fn hover(&mut self, p: &Json) -> Result<Json, Failure> {
        let Some((key, file, at)) = self.locate(p, true)? else { return Ok(Json::Null) };
        let unit = &self.units[&key];
        let index = unit.index.as_ref().expect("located units have an index");
        let lines = Lines::new(&index.files[file].text);
        if let Some(occ) = index.occ_at(file, at) {
            if let Some(info) = index.syms.get(&occ.sym) {
                return Ok(obj([
                    ("contents", obj([("kind", "markdown".into()), ("value", info.hover.clone().into())])),
                    ("range", lines.range(occ.span)),
                ]));
            }
        }
        match index.type_at(file, at) {
            Some((span, ty)) => Ok(obj([
                (
                    "contents",
                    obj([("kind", "markdown".into()), ("value", format!("```ante\n{ty}\n```").into())]),
                ),
                ("range", lines.range(span)),
            ])),
            None => Ok(Json::Null),
        }
    }

    fn sym_at(&mut self, p: &Json) -> Result<Option<(PathBuf, usize, Sym, Span)>, Failure> {
        let Some((key, file, at)) = self.locate(p, true)? else { return Ok(None) };
        let index = self.units[&key].index.as_ref().expect("located units have an index");
        Ok(index.occ_at(file, at).map(|o| (key, file, o.sym, o.span)))
    }

    fn definition(&mut self, p: &Json) -> Result<Json, Failure> {
        let Some((key, _, sym, _)) = self.sym_at(p)? else { return Ok(Json::Null) };
        let unit = &self.units[&key];
        let def = unit.index.as_ref().and_then(|ix| ix.syms.get(&sym)).and_then(|i| i.def);
        Ok(def.and_then(|(f, span)| self.location(unit, f, span)).unwrap_or(Json::Null))
    }

    fn references(&mut self, p: &Json) -> Result<Json, Failure> {
        let Some((key, _, sym, _)) = self.sym_at(p)? else { return Ok(Json::Null) };
        let with_decl = p.get("context").get("includeDeclaration").bool().unwrap_or(true);
        let unit = &self.units[&key];
        let index = unit.index.as_ref().expect("located units have an index");
        let def = index.syms.get(&sym).and_then(|i| i.def);
        let locations: Vec<Json> = index
            .occurrences(sym)
            .into_iter()
            .filter(|&(f, s)| with_decl || def != Some((f, s)))
            .filter_map(|(f, s)| self.location(unit, f, s))
            .collect();
        Ok(Json::Arr(locations))
    }

    /// What a rename of the symbol at `p` would touch, or why it can't.
    fn rename_target(&mut self, p: &Json) -> Result<(PathBuf, usize, Sym, Span), Failure> {
        let not_here = || (REQUEST_FAILED, "ikkada rename cheyyadaniki emi ledhu 🤷".to_string());
        let uri = p.get("textDocument").get("uri").str().unwrap_or("").to_string();
        let found = self.sym_at(p)?;
        let Some((key, file, sym, span)) = found else {
            // Nothing found on text the index doesn't match: the file
            // doesn't parse right now.
            let stale = self.docs.get(&uri).is_some_and(|d| {
                let unit = self.units.get(&unit_key(&d.path));
                let file =
                    unit.and_then(|u| u.paths.iter().position(|q| q.as_deref() == Some(d.path.as_path())));
                match (unit.and_then(|u| u.index.as_ref()), file) {
                    (Some(ix), Some(f)) => ix.files[f].text != d.text,
                    _ => true,
                }
            });
            return Err(if stale {
                (
                    REQUEST_FAILED,
                    "file lo errors unnayi. Mundhu avi fix chesi, appudu rename cheyyandi".into(),
                )
            } else {
                not_here()
            });
        };
        let index = self.units[&key].index.as_ref().expect("located units have an index");
        match index.syms.get(&sym) {
            Some(info) if info.renamable => {}
            Some(info) => {
                return Err((
                    REQUEST_FAILED,
                    format!(
                        "`{}` ni rename cheyyalem (standard library, trait method, leka `modhalu`)",
                        info.name
                    ),
                ))
            }
            None => return Err(not_here()),
        }
        // Every file it touches must be what the editor shows.
        let unit = &self.units[&key];
        for (f, _) in index.occurrences(sym) {
            let Some(path) = unit.paths.get(f).and_then(|p| p.as_ref()) else { continue };
            if let Some(doc) = self.docs.values().find(|d| &d.path == path) {
                if doc.text != index.files[f].text {
                    return Err((
                        REQUEST_FAILED,
                        "package lo errors unnayi. Mundhu avi fix chesi, appudu rename cheyyandi".into(),
                    ));
                }
            }
        }
        Ok((key, file, sym, span))
    }

    fn prepare_rename(&mut self, p: &Json) -> Result<Json, Failure> {
        let (key, file, _, span) = self.rename_target(p)?;
        let index = self.units[&key].index.as_ref().expect("located units have an index");
        Ok(Lines::new(&index.files[file].text).range(span))
    }

    fn rename(&mut self, p: &Json) -> Result<Json, Failure> {
        let new = p.get("newName").str().unwrap_or("").trim().to_string();
        if !is_identifier(&new) {
            return Err((
                INVALID_PARAMS,
                format!("`{new}` peru ga panikiraadhu. Letters, digits, `_` matrame; keyword kaakoodadhu"),
            ));
        }
        let (key, _, sym, _) = self.rename_target(p)?;
        let unit = &self.units[&key];
        let index = unit.index.as_ref().expect("located units have an index");
        let old = &index.syms[&sym].name;
        let mut changes: Vec<(String, Json)> = Vec::new();
        for (f, span) in index.occurrences(sym) {
            // An alias (`m ni k ga`) keeps its own name.
            if index.files[f].text.get(span.start..span.end) != Some(old.as_str()) {
                continue;
            }
            let Some(path) = unit.paths.get(f).and_then(|p| p.as_ref()) else { continue };
            let uri = self.uri_of(path);
            let edit = obj([
                ("range", Lines::new(&index.files[f].text).range(span)),
                ("newText", new.clone().into()),
            ]);
            match changes.iter_mut().find(|(u, _)| *u == uri) {
                Some((_, Json::Arr(edits))) => edits.push(edit),
                _ => changes.push((uri, Json::Arr(vec![edit]))),
            }
        }
        Ok(obj([("changes", Json::Obj(changes))]))
    }

    fn completion(&mut self, p: &Json) -> Result<Json, Failure> {
        let uri = p.get("textDocument").get("uri").str().unwrap_or("").to_string();
        let Some((key, file, at)) = self.locate(p, false)? else { return Ok(Json::Arr(keyword_items())) };
        let doc = &self.docs[&uri];
        let line = p.get("position").get("line").num().unwrap_or(0.0) as usize;
        let col = p.get("position").get("character").num().unwrap_or(0.0) as usize;
        let cursor = Lines::new(&doc.text).offset(line, col);
        let before = &doc.text[..cursor];
        let line_start = before.rfind(['\n', '\r']).map_or(0, |i| i + 1);
        let chain = dotted_chain(&before[line_start..]);
        let index = self.units[&key].index.as_ref().expect("located units have an index");
        let fx = &index.files[file];
        let mut items = Vec::new();
        match chain {
            Some(segments) => {
                let members = resolve_chain(index, file, at, &segments);
                items.extend(members.iter().map(|m| item(&m.name, m.kind, &m.detail)));
            }
            None => {
                let mut seen = HashSet::new();
                let mut locals: Vec<&index::LocalScope> =
                    fx.locals.iter().filter(|l| l.decl.end <= at && at <= l.until).collect();
                locals.sort_by_key(|l| std::cmp::Reverse(l.decl.start));
                for l in locals {
                    if seen.insert(l.name.clone()) {
                        items.push(item(&l.name, SymKind::Local, l.ty.as_deref().unwrap_or("")));
                    }
                }
                for m in &fx.globals {
                    if seen.insert(m.name.clone()) {
                        items.push(item(&m.name, m.kind, &m.detail));
                    }
                }
                for name in index.statics.keys() {
                    if crate::stdlib::STD_MODULES.iter().any(|s| s.name == name) && seen.insert(name.clone())
                    {
                        items.push(item(name, SymKind::Module, "module"));
                    }
                }
                items.extend(
                    keyword_items()
                        .into_iter()
                        .filter(|k| k.get("label").str().is_some_and(|l| !seen.contains(l))),
                );
            }
        }
        Ok(obj([("isIncomplete", false.into()), ("items", Json::Arr(items))]))
    }

    fn formatting(&mut self, p: &Json) -> Result<Json, Failure> {
        let uri = p.get("textDocument").get("uri").str().unwrap_or("");
        let Some(doc) = self.docs.get(uri) else { return Ok(Json::Null) };
        let src = SourceFile::new(doc.path.display().to_string(), doc.text.clone());
        match crate::fmt::format_source(&src) {
            Ok(text) if text == doc.text => Ok(Json::Arr(Vec::new())),
            Ok(text) => {
                Ok(Json::Arr(vec![obj([("range", Lines::new(&doc.text).whole()), ("newText", text.into())])]))
            }
            // Errors are already showing; there's nothing to format yet.
            Err(crate::fmt::FmtError::Invalid(_)) => Ok(Json::Null),
            Err(crate::fmt::FmtError::Bug(b)) => Err((INTERNAL, format!("antec fmt lo bug 🐛: {b}"))),
        }
    }
}

fn error_reply(id: Json, code: i64, message: String) -> Json {
    obj([
        ("jsonrpc", "2.0".into()),
        ("id", id),
        ("error", obj([("code", code.into()), ("message", message.into())])),
    ])
}

fn capabilities() -> Json {
    obj([
        (
            "capabilities",
            obj([
                (
                    "textDocumentSync",
                    obj([
                        ("openClose", true.into()),
                        ("change", 1usize.into()),
                        ("save", obj([("includeText", false.into())])),
                    ]),
                ),
                ("hoverProvider", true.into()),
                ("definitionProvider", true.into()),
                ("referencesProvider", true.into()),
                ("renameProvider", obj([("prepareProvider", true.into())])),
                ("completionProvider", obj([("triggerCharacters", Json::Arr(vec![".".into()]))])),
                ("documentFormattingProvider", true.into()),
            ]),
        ),
        ("serverInfo", obj([("name", "antec".into()), ("version", env!("CARGO_PKG_VERSION").into())])),
    ])
}

/// The unit a document belongs to: its package root when it sits in the
/// package's `src/`, else the file itself.
fn unit_key(path: &Path) -> PathBuf {
    if let Some(root) = crate::package::find_package_root(path) {
        if canonical(path).starts_with(root.join("src")) {
            return root;
        }
    }
    path.to_path_buf()
}

fn is_identifier(name: &str) -> bool {
    let lexed = crate::lexer::lex(&SourceFile::new("<peru>", name));
    let words: Vec<_> = lexed
        .tokens
        .iter()
        .filter(|t| {
            !matches!(
                t.kind,
                crate::token::TokenKind::Newline
                    | crate::token::TokenKind::Indent
                    | crate::token::TokenKind::Dedent
                    | crate::token::TokenKind::Eof
            )
        })
        .collect();
    lexed.diagnostics.is_empty()
        && words.len() == 1
        && matches!(&words[0].kind, crate::token::TokenKind::Ident(n) if n == name)
}

/// `a.b.` (or `a.b.pre`) before the cursor: `["a", "b"]`. `None` when the
/// cursor isn't after a dot.
fn dotted_chain(line: &str) -> Option<Vec<String>> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let rest = line.trim_end_matches(is_word);
    let mut rest = rest.strip_suffix('.')?;
    let mut segments = Vec::new();
    loop {
        let word_start = rest.trim_end_matches(is_word).len();
        let word = &rest[word_start..];
        if word.is_empty() {
            break;
        }
        segments.push(word.to_string());
        rest = &rest[..word_start];
        match rest.strip_suffix('.') {
            Some(r) => rest = r,
            None => break,
        }
    }
    segments.reverse();
    (!segments.is_empty()).then_some(segments)
}

/// The members offered after `segments.` at `at`.
fn resolve_chain<'i>(index: &'i Index, file: usize, at: usize, segments: &[String]) -> &'i [index::Member] {
    let fx = &index.files[file];
    let first = &segments[0];
    let mut members: &[index::Member] = if first == "naa" {
        let scope = fx
            .fns
            .iter()
            .filter(|f| f.body.start <= at && at <= f.body.end)
            .min_by_key(|f| f.body.end - f.body.start);
        match scope.and_then(|f| f.naa.as_ref()).and_then(|k| index.members.get(k)) {
            Some(m) => m,
            None => return &[],
        }
    } else {
        let local = fx
            .locals
            .iter()
            .filter(|l| &l.name == first && l.decl.end <= at && at <= l.until)
            .max_by_key(|l| l.decl.start);
        match local {
            Some(l) => match l.ty.as_ref().and_then(|k| index.members.get(k)) {
                Some(m) => m,
                None => return &[],
            },
            None => match index.statics.get(first) {
                Some(m) => m,
                None => return &[],
            },
        }
    };
    for seg in &segments[1..] {
        let next = members.iter().find(|m| &m.name == seg);
        members = match next {
            Some(m) => match m.ty.as_ref().and_then(|k| index.members.get(k)) {
                Some(ms) => ms,
                None => index.statics.get(seg).map_or(&[], |v| v.as_slice()),
            },
            None => index.statics.get(seg).map_or(&[], |v| v.as_slice()),
        };
    }
    members
}

fn item(label: &str, kind: SymKind, detail: &str) -> Json {
    // LSP `CompletionItemKind` numbers.
    let kind: usize = match kind {
        SymKind::Method => 2,
        SymKind::Function => 3,
        SymKind::Field => 5,
        SymKind::Local => 6,
        SymKind::Trait => 8,
        SymKind::Module => 9,
        SymKind::Enum => 13,
        SymKind::Variant => 20,
        SymKind::Const => 21,
        SymKind::Record => 22,
        SymKind::Other => 1,
    };
    let mut fields = vec![("label".to_string(), Json::from(label)), ("kind".to_string(), kind.into())];
    if !detail.is_empty() {
        fields.push(("detail".to_string(), detail.into()));
    }
    Json::Obj(fields)
}

fn keyword_items() -> Vec<Json> {
    crate::token::Keyword::ALL
        .iter()
        .filter(|k| !k.is_reserved_for_later() && !k.is_planned())
        .map(|k| obj([("label", k.as_str().into()), ("kind", 14usize.into())]))
        .collect()
}

fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// The path of a `file:` URI.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // Skip an authority (`file://host/…`); local files have none.
    let rest = &rest[rest.find('/')?..];
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(b) = rest.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    let mut path = String::from_utf8(out).ok()?;
    // `/c:/x` on Windows.
    if cfg!(windows) && path.len() > 2 && path.as_bytes()[2] == b':' {
        path.remove(0);
    }
    Some(PathBuf::from(path))
}

/// A `file:` URI for a path, percent-encoding what URIs don't allow.
pub fn path_to_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    if !text.starts_with('/') {
        out.push('/');
    }
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris() {
        let uri = "file:///Volumes/T7%20Shield/%E0%B0%85.ante";
        let path = uri_to_path(uri).unwrap();
        assert_eq!(path, PathBuf::from("/Volumes/T7 Shield/అ.ante"));
        assert_eq!(path_to_uri(&path), uri);
        assert_eq!(uri_to_path("untitled:Untitled-1"), None);
    }

    #[test]
    fn chains() {
        assert_eq!(dotted_chain("    x ante naa.a.en"), Some(vec!["naa".into(), "a".into()]));
        assert_eq!(dotted_chain("p."), Some(vec!["p".into()]));
        assert_eq!(dotted_chain("p"), None);
        assert_eq!(dotted_chain("(f())."), None);
    }

    #[test]
    fn identifiers() {
        assert!(is_identifier("kotha_peru"));
        assert!(!is_identifier("ante"));
        assert!(!is_identifier("a b"));
        assert!(!is_identifier("1a"));
        assert!(!is_identifier(""));
    }
}
