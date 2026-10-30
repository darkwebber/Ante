// The language client side: the extension finds `antec`, starts quietly
// without it (highlighting only), and, when an `antec` is around, speaks
// LSP with it through the same protocol library VS Code's client uses.

const fs = require("fs");
const os = require("os");
const path = require("path");
const assert = require("assert");
const Module = require("module");
const cp = require("child_process");
const { findAntec } = require("../server-path");

const root = path.join(__dirname, "..");
let failures = 0;
const tests = [];
const test = (name, fn) => tests.push({ name, fn });

// ── package.json wiring ──────────────────────────────────────────────────

test("package.json starts the client for .ante files", () => {
  const pkg = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8"));
  assert.strictEqual(pkg.main, "./extension.js");
  assert.ok(fs.existsSync(path.join(root, pkg.main)));
  assert.ok(pkg.activationEvents.includes("onLanguage:ante"));
  assert.ok(pkg.dependencies["vscode-languageclient"], "the client ships as a dependency");
  const props = pkg.contributes.configuration.properties;
  assert.strictEqual(props["ante.antecPath"].default, "antec");
  assert.strictEqual(props["ante.lsp.enabled"].default, true);
  assert.ok(props["ante.trace.server"], "LanguageClient reads <id>.trace.server");
  assert.ok(pkg.contributes.commands.some((c) => c.command === "ante.restartServer"));
  const ignore = fs.readFileSync(path.join(root, ".vscodeignore"), "utf8");
  assert.ok(!/^node_modules\/\*\*/m.test(ignore), "the .vsix must keep the client library");
});

// ── finding antec ────────────────────────────────────────────────────────

function fakeBin(dir, name) {
  fs.mkdirSync(dir, { recursive: true });
  const p = path.join(dir, name);
  fs.writeFileSync(p, "#!/bin/sh\n");
  fs.chmodSync(p, 0o755);
  return p;
}

test("findAntec: PATH, ~/.cargo/bin, explicit and relative paths", () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "ante-vscode-"));
  try {
    const onPath = fakeBin(path.join(tmp, "bin"), "antec");
    const env = { PATH: ["/nowhere", path.join(tmp, "bin")].join(":"), HOME: path.join(tmp, "home") };
    assert.strictEqual(findAntec("antec", { env, platform: "linux" }), onPath);
    assert.strictEqual(findAntec("", { env, platform: "linux" }), onPath, "empty means the default");

    const cargo = fakeBin(path.join(tmp, "home", ".cargo", "bin"), "antec");
    assert.strictEqual(findAntec("antec", { env: { PATH: "/nowhere", HOME: env.HOME }, platform: "linux" }), cargo);

    assert.strictEqual(findAntec(onPath, { env: {}, platform: "linux" }), onPath);
    const built = fakeBin(path.join(tmp, "ws", "target", "release"), "antec");
    assert.strictEqual(findAntec("target/release/antec", { env: {}, platform: "linux", base: path.join(tmp, "ws") }), built);
    assert.strictEqual(findAntec("target/release/antec", { env: {}, platform: "linux" }), null, "relative needs a folder");
    assert.strictEqual(findAntec("~/.cargo/bin/antec", { env: { HOME: env.HOME }, platform: "linux" }), cargo);

    assert.strictEqual(findAntec("antec", { env: { PATH: "/nowhere" }, platform: "linux" }), null);
    assert.strictEqual(findAntec(path.join(tmp, "ledhu"), { env: {}, platform: "linux" }), null);

    const exe = fakeBin(path.join(tmp, "win"), "antec.exe");
    assert.strictEqual(findAntec("antec", { env: { PATH: path.join(tmp, "win"), PATHEXT: ".EXE" }, platform: "win32" }), exe);
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});

// ── activation without antec ─────────────────────────────────────────────

/** Loads extension.js against a stub `vscode` module. */
function loadExtension(config) {
  const calls = { info: [], warn: [], commands: [] };
  const vscode = {
    workspace: {
      getConfiguration: () => ({ get: (k, d) => (k in config ? config[k] : d) }),
      onDidChangeConfiguration: () => ({ dispose() {} }),
      workspaceFolders: undefined,
    },
    window: {
      showInformationMessage: (...a) => {
        calls.info.push(a);
        return Promise.resolve(undefined);
      },
      showWarningMessage: (...a) => {
        calls.warn.push(a);
        return Promise.resolve(undefined);
      },
    },
    commands: {
      registerCommand: (name) => {
        calls.commands.push(name);
        return { dispose() {} };
      },
      executeCommand: () => Promise.resolve(),
    },
    env: { openExternal: () => Promise.resolve(true) },
    Uri: { parse: (s) => s },
  };
  const resolve = Module._resolveFilename;
  Module._resolveFilename = function (request, ...rest) {
    return request === "vscode" ? "vscode" : resolve.call(this, request, ...rest);
  };
  require.cache.vscode = { id: "vscode", filename: "vscode", loaded: true, exports: vscode };
  const file = require.resolve("../extension.js");
  delete require.cache[file];
  const ext = require(file);
  Module._resolveFilename = resolve;
  return { ext, calls };
}

function context() {
  const state = new Map();
  return {
    subscriptions: [],
    globalState: { get: (k) => state.get(k), update: (k, v) => Promise.resolve(state.set(k, v)) },
  };
}

test("without antec: activates, says so once, highlighting stays", async () => {
  const { ext, calls } = loadExtension({ antecPath: "/ledhu/antec" });
  const ctx = context();
  await ext.activate(ctx);
  assert.strictEqual(calls.info.length, 1, "one gentle message");
  assert.match(calls.info[0][0], /dorakaledhu/);
  assert.strictEqual(calls.warn.length, 0, "no warnings");
  assert.ok(calls.commands.includes("ante.restartServer"));
  assert.ok(ctx.subscriptions.length >= 2);
  await ext.deactivate();

  // "Malli cheppaku" keeps it quiet afterwards.
  await ctx.globalState.update("ante.missingAntec.quiet", true);
  const again = loadExtension({ antecPath: "/ledhu/antec" });
  await again.ext.activate(ctx);
  assert.strictEqual(again.calls.info.length, 0);
});

test("lsp.enabled = false: no server, no message", async () => {
  const { ext, calls } = loadExtension({ "lsp.enabled": false, antecPath: "/ledhu/antec" });
  await ext.activate(context());
  assert.strictEqual(calls.info.length + calls.warn.length, 0);
});

// ── a real handshake, when antec is around ───────────────────────────────

test("speaks LSP with antec (skipped without one)", async () => {
  const antec = findAntec(process.env.ANTEC || "antec");
  if (!antec) {
    console.log("    (antec dorakaledhu: skip. `ANTEC=/path/to/antec npm test` tho run cheyyandi)");
    return;
  }
  const rpc = require("vscode-jsonrpc/node");
  const proc = cp.spawn(antec, ["lsp"], { stdio: ["pipe", "pipe", "inherit"] });
  const conn = rpc.createMessageConnection(
    new rpc.StreamMessageReader(proc.stdout),
    new rpc.StreamMessageWriter(proc.stdin),
  );
  const diagnostics = new Promise((resolve) => {
    conn.onNotification("textDocument/publishDiagnostics", (p) => resolve(p));
  });
  conn.listen();
  try {
    const init = await conn.sendRequest("initialize", { processId: process.pid, rootUri: null, capabilities: {} });
    assert.strictEqual(init.serverInfo.name, "antec");
    assert.ok(init.capabilities.completionProvider);
    conn.sendNotification("initialized", {});
    const uri = "untitled:Untitled-1";
    const text = 'modhalu() -> khali ante:\n    "తెలుగు 🚀" ni choopinchu()\n    ledhu_peru ni choopinchu()\n';
    conn.sendNotification("textDocument/didOpen", { textDocument: { uri, languageId: "ante", version: 1, text } });
    const d = await diagnostics;
    assert.strictEqual(d.uri, uri);
    assert.strictEqual(d.diagnostics[0].code, "E0301");
    assert.deepStrictEqual(d.diagnostics[0].range.start, { line: 2, character: 4 });
    const hover = await conn.sendRequest("textDocument/hover", { textDocument: { uri }, position: { line: 1, character: 20 } });
    assert.match(hover.contents.value, /choopinchu/);
    await conn.sendRequest("shutdown");
    conn.sendNotification("exit");
    const code = await new Promise((resolve) => proc.on("exit", resolve));
    assert.strictEqual(code, 0);
  } finally {
    conn.dispose();
    proc.kill();
  }
});

(async () => {
  for (const t of tests) {
    try {
      await t.fn();
      console.log(`  ✓ ${t.name}`);
    } catch (err) {
      failures++;
      console.log(`  ✗ ${t.name}\n${err.stack}`);
    }
  }
  if (failures) {
    console.log(`\n${failures} test(s) fail ayyayi`);
    process.exit(1);
  }
  console.log(`\nclient tests anni pass ✅`);
})();
