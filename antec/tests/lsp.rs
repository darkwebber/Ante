//! `antec lsp`: drives the language server over stdio with scripted
//! messages, the way an editor does.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use antec::lsp::json::{obj, Json};
use antec::lsp::path_to_uri;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("antec-lsp-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    dir.canonicalize().expect("temp dir path")
}

struct Lsp {
    child: Child,
    stdin: ChildStdin,
    out: std::sync::mpsc::Receiver<Json>,
    next: i64,
    /// Notifications seen while waiting for replies.
    notes: Vec<Json>,
}

fn read_frame(r: &mut BufReader<ChildStdout>) -> Option<Json> {
    let mut len = 0;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(v) = line.strip_prefix("Content-Length:") {
            len = v.trim().parse().ok()?;
        }
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body).ok()?;
    Some(Json::parse(std::str::from_utf8(&body).expect("UTF-8 reply")).expect("JSON reply"))
}

impl Lsp {
    fn start() -> Lsp {
        let mut child = Command::new(env!("CARGO_BIN_EXE_antec"))
            .arg("lsp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("antec lsp starts");
        let stdin = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let (tx, out) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            while let Some(m) = read_frame(&mut stdout) {
                if tx.send(m).is_err() {
                    return;
                }
            }
        });
        let mut lsp = Lsp { child, stdin, out, next: 1, notes: Vec::new() };
        let init = lsp.request("initialize", obj([("capabilities", obj([]))]));
        assert_eq!(init.get("serverInfo").get("name").str(), Some("antec"));
        assert_eq!(init.get("capabilities").get("hoverProvider").bool(), Some(true));
        lsp.notify("initialized", obj([]));
        lsp
    }

    fn send(&mut self, msg: Json) {
        let body = msg.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        self.stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str, params: Json) {
        self.send(obj([("jsonrpc", "2.0".into()), ("method", method.into()), ("params", params)]));
    }

    fn recv(&mut self) -> Json {
        self.out.recv_timeout(Duration::from_secs(30)).expect("antec lsp answers")
    }

    /// The whole reply (`result` or `error`) to a request.
    fn call(&mut self, method: &str, params: Json) -> Json {
        let id = self.next;
        self.next += 1;
        self.send(obj([
            ("jsonrpc", "2.0".into()),
            ("id", id.into()),
            ("method", method.into()),
            ("params", params),
        ]));
        loop {
            let m = self.recv();
            if m.get("id").num() == Some(id as f64) {
                return m;
            }
            self.notes.push(m);
        }
    }

    fn request(&mut self, method: &str, params: Json) -> Json {
        let reply = self.call(method, params);
        assert!(reply.get("error").is_null(), "{method} failed: {reply}");
        reply.get("result").clone()
    }

    /// The next diagnostics published for `uri`.
    fn diagnostics(&mut self, uri: &str) -> Vec<Json> {
        if let Some(i) = self.notes.iter().position(|n| is_diags_for(n, uri)) {
            return self.notes.remove(i).get("params").get("diagnostics").arr().to_vec();
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            let m = self.recv();
            if is_diags_for(&m, uri) {
                return m.get("params").get("diagnostics").arr().to_vec();
            }
            self.notes.push(m);
        }
        panic!("no diagnostics for {uri}");
    }

    fn open(&mut self, path: &Path, text: &str) -> String {
        let uri = path_to_uri(path);
        self.notify(
            "textDocument/didOpen",
            obj([(
                "textDocument",
                obj([
                    ("uri", uri.clone().into()),
                    ("languageId", "ante".into()),
                    ("version", 1usize.into()),
                    ("text", text.into()),
                ]),
            )]),
        );
        uri
    }

    fn change(&mut self, uri: &str, version: usize, text: &str) {
        self.notify(
            "textDocument/didChange",
            obj([
                ("textDocument", obj([("uri", uri.into()), ("version", version.into())])),
                ("contentChanges", Json::Arr(vec![obj([("text", text.into())])])),
            ]),
        );
    }

    fn at(&mut self, method: &str, uri: &str, (line, col): (usize, usize), extra: Vec<(&str, Json)>) -> Json {
        let mut params = vec![
            ("textDocument".to_string(), obj([("uri", uri.into())])),
            ("position".to_string(), obj([("line", line.into()), ("character", col.into())])),
        ];
        params.extend(extra.into_iter().map(|(k, v)| (k.to_string(), v)));
        self.call(method, Json::Obj(params))
    }

    fn finish(mut self) {
        assert!(self.request("shutdown", Json::Null).is_null());
        self.notify("exit", Json::Null);
        let status = self.child.wait().expect("antec lsp exits");
        assert!(status.success(), "exit after shutdown is 0, got {status}");
    }
}

fn is_diags_for(m: &Json, uri: &str) -> bool {
    m.get("method").str() == Some("textDocument/publishDiagnostics")
        && m.get("params").get("uri").str() == Some(uri)
}

/// `(line, utf16 column)` of the `nth` occurrence of `needle` in `text`,
/// plus `skip` characters.
fn pos(text: &str, needle: &str, nth: usize, skip: usize) -> (usize, usize) {
    let at = text.match_indices(needle).nth(nth).unwrap_or_else(|| panic!("`{needle}` #{nth} in text")).0;
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line = text[..at].matches('\n').count();
    let prefix: String = text[line_start..at].chars().chain(text[at..].chars().take(skip)).collect();
    (line, prefix.encode_utf16().count())
}

fn range_start(r: &Json) -> (usize, usize) {
    let s = r.get("range").get("start");
    (s.get("line").num().unwrap() as usize, s.get("character").num().unwrap() as usize)
}

fn hover_text(reply: &Json) -> String {
    reply.get("result").get("contents").get("value").str().unwrap_or("").to_string()
}

const PROGRAM: &str = "\
# తెలుగు comment 🚀: positions LSP lo UTF-16 lo lekkapedathayi
rakam Bindhu ante:
    x: ankam
    y: ankam

edho_okati Rangu ante:
    Erupu
    Pacha

# Rendu numbers kalupu.
kalupu(a: ankam, b: ankam tho) -> ankam ante:
    (a + b) icchey

Bindhu panulu ante:
    motham(naa: &idhi) -> ankam ante:
        (naa.x, naa.y tho kalupu()) icchey

modhalu() -> khali ante:
    peru ante \"తెలుగు 🚀\"
    p ante Bindhu{x: 1, y: 2}
    _r ante Rangu.Pacha
    motham ante p.x, p.y tho kalupu()
    f\"{peru} {motham} {p.motham()}\" ni choopinchu()
";

#[test]
fn a_whole_editing_session() {
    let dir = scratch("session");
    let path = dir.join("bindhu.ante");
    fs::write(&path, PROGRAM).unwrap();
    let mut lsp = Lsp::start();

    // An error right after Telugu text and an emoji: its column counts
    // UTF-16 units (`తెలుగు` is 6, `🚀` is 2), not bytes or characters.
    let broken = PROGRAM.replace("peru ante \"తెలుగు 🚀\"", "peru ante (\"తెలుగు 🚀\", theliyani)");
    let uri = lsp.open(&path, &broken);
    let diags = lsp.diagnostics(&uri);
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].get("code").str(), Some("E0301"));
    assert_eq!(diags[0].get("severity").num(), Some(1.0));
    assert_eq!(range_start(&diags[0]), pos(&broken, "theliyani", 0, 0));
    let end = diags[0].get("range").get("end");
    assert_eq!(end.get("character").num(), Some((pos(&broken, "theliyani", 0, 0).1 + 9) as f64));

    // Hover still answers (types are checked anyway) and doesn't crash.
    let h = lsp.at("textDocument/hover", &uri, pos(&broken, "p ante", 0, 0), vec![]);
    assert!(h.get("error").is_null(), "{h}");

    // Fixing it clears the diagnostics after the typing pause.
    lsp.change(&uri, 2, PROGRAM);
    let diags = lsp.diagnostics(&uri);
    assert!(diags.is_empty(), "{diags:?}");

    // Hover: a local's type, a function's signature with its doc comment,
    // a field, a variant, a method.
    let text = PROGRAM;
    let h = lsp.at("textDocument/hover", &uri, pos(text, "motham ante", 0, 1), vec![]);
    assert!(hover_text(&h).contains("motham: ankam"), "{h}");
    let h = lsp.at("textDocument/hover", &uri, pos(text, "kalupu()", 1, 2), vec![]);
    let t = hover_text(&h);
    assert!(
        t.contains("kalupu(a: ankam, b: ankam tho) -> ankam") && t.contains("Rendu numbers kalupu."),
        "{t}"
    );
    let h = lsp.at("textDocument/hover", &uri, pos(text, "p.y", 0, 2), vec![]);
    assert!(hover_text(&h).contains("y: ankam"), "{h}");
    let h = lsp.at("textDocument/hover", &uri, pos(text, "Rangu.Pacha", 0, 7), vec![]);
    assert!(hover_text(&h).contains("Rangu.Pacha"), "{h}");
    let h = lsp.at("textDocument/hover", &uri, pos(text, "p.motham", 0, 3), vec![]);
    assert!(hover_text(&h).contains("motham(naa: &idhi) -> ankam"), "{h}");
    // On Telugu text inside a string: the string's type.
    let h = lsp.at("textDocument/hover", &uri, pos(text, "లుగు", 1, 1), vec![]);
    assert!(hover_text(&h).contains("padham"), "{h}");
    assert_eq!(range_start(h.get("result")), pos(text, "\"తెలుగు 🚀\"", 0, 0));

    // Definition: a call, a field through `naa`, a local, a type.
    let d = lsp.at("textDocument/definition", &uri, pos(text, "kalupu()", 1, 0), vec![]);
    assert_eq!(range_start(d.get("result")), pos(text, "kalupu(a", 0, 0));
    let d = lsp.at("textDocument/definition", &uri, pos(text, "naa.y", 0, 4), vec![]);
    assert_eq!(range_start(d.get("result")), pos(text, "y: ankam", 0, 0));
    let d = lsp.at("textDocument/definition", &uri, pos(text, "{peru}", 0, 1), vec![]);
    assert_eq!(range_start(d.get("result")), pos(text, "peru ante", 0, 0));
    let d = lsp.at("textDocument/definition", &uri, pos(text, "Bindhu{", 0, 0), vec![]);
    assert_eq!(range_start(d.get("result")), pos(text, "Bindhu ante", 0, 0));
    assert_eq!(d.get("result").get("uri").str(), Some(uri.as_str()));

    // References to field `x`: its declaration, `naa.x`, `Bindhu{x: …}`, `p.x`.
    let r = lsp.at(
        "textDocument/references",
        &uri,
        pos(text, "x: ankam", 0, 0),
        vec![("context", obj([("includeDeclaration", true.into())]))],
    );
    let starts: Vec<_> = r.get("result").arr().iter().map(range_start).collect();
    assert_eq!(
        starts,
        vec![
            pos(text, "x: ankam", 0, 0),
            pos(text, "naa.x", 0, 4),
            pos(text, "x: 1", 0, 0),
            pos(text, "p.x", 0, 2)
        ]
    );
    let r = lsp.at(
        "textDocument/references",
        &uri,
        pos(text, "x: ankam", 0, 0),
        vec![("context", obj([("includeDeclaration", false.into())]))],
    );
    assert_eq!(r.get("result").arr().len(), 3);

    // Rename a local, then a function; a keyword is refused.
    let e = lsp.at("textDocument/rename", &uri, pos(text, "{peru}", 0, 1), vec![("newName", "perlu".into())]);
    let edits = e.get("result").get("changes").get(&uri).arr().to_vec();
    assert_eq!(edits.len(), 2, "{e}");
    assert!(edits.iter().all(|t| t.get("newText").str() == Some("perlu")));
    let e =
        lsp.at("textDocument/rename", &uri, pos(text, "kalupu(a", 0, 0), vec![("newName", "koodu".into())]);
    assert_eq!(e.get("result").get("changes").get(&uri).arr().len(), 3, "{e}");
    let e =
        lsp.at("textDocument/rename", &uri, pos(text, "kalupu(a", 0, 0), vec![("newName", "ante".into())]);
    assert!(!e.get("error").is_null(), "{e}");
    let e = lsp.at("textDocument/prepareRename", &uri, pos(text, "choopinchu", 0, 0), vec![]);
    assert!(!e.get("error").is_null(), "the prelude can't be renamed: {e}");
    let e = lsp.at("textDocument/prepareRename", &uri, pos(text, "motham ante", 0, 0), vec![]);
    // prepareRename answers with a bare range.
    assert_eq!(range_start(&obj([("range", e.get("result").clone())])), pos(text, "motham ante", 0, 0));

    // Completion right after typing `p.` (the file doesn't parse yet):
    // fields and methods of `Bindhu`. And `naa.` inside the method.
    let typing = PROGRAM.replace("    motham ante p.x", "    q ante p.\n    motham ante p.x");
    lsp.change(&uri, 3, &typing);
    let c = lsp.at("textDocument/completion", &uri, pos(&typing, "p.\n", 0, 2), vec![]);
    let labels: Vec<&str> =
        c.get("result").get("items").arr().iter().filter_map(|i| i.get("label").str()).collect();
    for want in ["x", "y", "motham"] {
        assert!(labels.contains(&want), "`{want}` in {labels:?}");
    }
    let typing2 = PROGRAM.replace("        (naa.x", "        naa.\n        (naa.x");
    lsp.change(&uri, 4, &typing2);
    let c = lsp.at("textDocument/completion", &uri, pos(&typing2, "naa.\n", 0, 4), vec![]);
    let labels: Vec<&str> =
        c.get("result").get("items").arr().iter().filter_map(|i| i.get("label").str()).collect();
    assert!(labels.contains(&"x") && labels.contains(&"motham"), "{labels:?}");
    // Errors show for the half-typed line, and hover on stale text says nothing.
    assert!(!lsp.diagnostics(&uri).is_empty());
    let h = lsp.at("textDocument/hover", &uri, pos(&typing2, "kalupu()", 1, 0), vec![]);
    assert!(h.get("result").is_null(), "{h}");

    // Plain completion: locals in scope, file items, prelude names, the
    // "Ante" prelude's containers, keywords.
    lsp.change(&uri, 5, PROGRAM);
    let c = lsp.at("textDocument/completion", &uri, pos(text, "f\"{peru}", 0, 0), vec![]);
    let labels: Vec<&str> =
        c.get("result").get("items").arr().iter().filter_map(|i| i.get("label").str()).collect();
    for want in ["peru", "p", "_r", "motham", "kalupu", "Bindhu", "choopinchu", "kuppa", "aithe", "icchey"] {
        assert!(labels.contains(&want), "`{want}` in {labels:?}");
    }
    // `Rangu.` offers its variants.
    let typing3 = PROGRAM.replace("    _r ante Rangu.Pacha", "    _r ante Rangu.");
    lsp.change(&uri, 6, &typing3);
    let c = lsp.at("textDocument/completion", &uri, pos(&typing3, "Rangu.\n", 0, 6), vec![]);
    let labels: Vec<&str> =
        c.get("result").get("items").arr().iter().filter_map(|i| i.get("label").str()).collect();
    assert!(labels.contains(&"Erupu") && labels.contains(&"Pacha"), "{labels:?}");

    // Formatting: one edit with the formatted text; none when already tidy.
    let messy = PROGRAM.replace("    (a + b) icchey", "    (a+b)   icchey");
    lsp.change(&uri, 7, &messy);
    let f = lsp.call(
        "textDocument/formatting",
        obj([
            ("textDocument", obj([("uri", uri.clone().into())])),
            ("options", obj([("tabSize", 4usize.into()), ("insertSpaces", true.into())])),
        ]),
    );
    let edits = f.get("result").arr();
    assert_eq!(edits.len(), 1, "{f}");
    assert_eq!(edits[0].get("newText").str(), Some(PROGRAM));
    lsp.change(&uri, 8, PROGRAM);
    let f = lsp.call("textDocument/formatting", obj([("textDocument", obj([("uri", uri.clone().into())]))]));
    assert!(f.get("result").arr().is_empty(), "{f}");

    // Unknown requests get an error, not silence.
    let u = lsp.call("textDocument/inlayHint", obj([]));
    assert_eq!(u.get("error").get("code").num(), Some(-32601.0));
    lsp.finish();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_package_across_files() {
    let dir = scratch("package");
    fs::write(dir.join("ante.toml"), "[package]\nname = \"chinna\"\nversion = \"0.1.0\"\n").unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    let lekka = "andhariki rettimpu(n: ankam ni) -> ankam ante:\n    (n * 2) icchey\n";
    let main = "lekka nunchi rettimpu thechuko\n\nmodhalu() -> khali ante:\n    (21 ni rettimpu()) ni choopinchu()\n";
    fs::write(dir.join("src/lekka.ante"), lekka).unwrap();
    fs::write(dir.join("src/modhalu.ante"), main).unwrap();
    let mut lsp = Lsp::start();
    let main_uri = lsp.open(&dir.join("src/modhalu.ante"), main);
    let lekka_uri = path_to_uri(&dir.join("src/lekka.ante"));
    assert!(lsp.diagnostics(&main_uri).is_empty());

    // Definition jumps into the other file.
    let d = lsp.at("textDocument/definition", &main_uri, pos(main, "rettimpu()", 0, 0), vec![]);
    assert_eq!(d.get("result").get("uri").str(), Some(lekka_uri.as_str()), "{d}");
    assert_eq!(range_start(d.get("result")), (0, "andhariki ".len()));

    // Rename touches both files, the import included.
    let e = lsp.at(
        "textDocument/rename",
        &main_uri,
        pos(main, "rettimpu()", 0, 0),
        vec![("newName", "double".into())],
    );
    let changes = e.get("result").get("changes");
    assert_eq!(changes.get(&main_uri).arr().len(), 2, "{e}");
    assert_eq!(changes.get(&lekka_uri).arr().len(), 1, "{e}");

    // An unsaved edit in one file shows its error in the other.
    let lekka_bad = lekka.replace("andhariki ", "");
    let lekka_uri2 = lsp.open(&dir.join("src/lekka.ante"), &lekka_bad);
    assert_eq!(lekka_uri2, lekka_uri);
    let diags = loop {
        let d = lsp.diagnostics(&main_uri);
        if !d.is_empty() {
            break d;
        }
    };
    assert_eq!(range_start(&diags[0]), pos(main, "rettimpu", 0, 0), "{diags:?}");
    lsp.finish();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn broken_input_never_crashes_it() {
    let dir = scratch("broken");
    let mut lsp = Lsp::start();
    let texts = [
        "modhalu() -> khali ante:\n    x ante (1 +\n",
        "rakam ante ante ante:::\n\t\t?",
        "modhalu() -> khali ante:\n    \"తెలుగు",
        "",
        "modhalu() -> khali ante:\n    x ante 1\n    x.y.z ni choopinchu()\n",
    ];
    for (i, text) in texts.iter().enumerate() {
        let path = dir.join(format!("b{i}.ante"));
        let uri = lsp.open(&path, text);
        let _ = lsp.diagnostics(&uri);
        for (line, col) in [(0, 0), (1, 5), (1, 999), (99, 0)] {
            for method in ["textDocument/hover", "textDocument/definition", "textDocument/completion"] {
                let r = lsp.at(method, &uri, (line, col), vec![]);
                assert!(r.get("error").is_null(), "{method} on {text:?}: {r}");
            }
        }
    }
    // A request for a file that isn't open.
    let r = lsp.at("textDocument/hover", "file:///ledhu.ante", (0, 0), vec![]);
    assert!(r.get("result").is_null());
    lsp.finish();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn exit_without_shutdown_is_an_error() {
    let mut lsp = Lsp::start();
    lsp.notify("exit", Json::Null);
    assert_eq!(lsp.child.wait().unwrap().code(), Some(1));
}
