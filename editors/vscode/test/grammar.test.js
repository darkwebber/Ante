// Tokenizes "Ante" lines with the real TextMate engine VS Code uses and
// checks the scopes of key tokens. Also checks that every keyword the
// compiler knows gets highlighted, so grammar and compiler can't drift.

const fs = require("fs");
const path = require("path");
const assert = require("assert");
const oniguruma = require("vscode-oniguruma");
const vsctm = require("vscode-textmate");

const root = path.join(__dirname, "..");

async function loadGrammar() {
  const wasm = fs.readFileSync(require.resolve("vscode-oniguruma/release/onig.wasm")).buffer;
  await oniguruma.loadWASM(wasm);
  const registry = new vsctm.Registry({
    onigLib: Promise.resolve({
      createOnigScanner: (patterns) => new oniguruma.OnigScanner(patterns),
      createOnigString: (s) => new oniguruma.OnigString(s),
    }),
    loadGrammar: async (scopeName) => {
      if (scopeName !== "source.ante") return null;
      const text = fs.readFileSync(path.join(root, "syntaxes/ante.tmLanguage.json"), "utf8");
      return vsctm.parseRawGrammar(text, "ante.tmLanguage.json");
    },
  });
  return registry.loadGrammar("source.ante");
}

function tokenize(grammar, line) {
  const { tokens } = grammar.tokenizeLine(line, vsctm.INITIAL);
  return tokens.map((t) => ({ text: line.slice(t.startIndex, t.endIndex), scopes: t.scopes }));
}

/** Asserts the nth occurrence of `text` in `line` carries `scope`. */
function expectScope(grammar, line, text, scope, nth = 0) {
  const tokens = tokenize(grammar, line);
  const matches = tokens.filter((t) => t.text === text);
  assert.ok(matches[nth], `no token ${JSON.stringify(text)} in ${JSON.stringify(line)}: ${JSON.stringify(tokens.map((t) => t.text))}`);
  const scopes = matches[nth].scopes;
  assert.ok(
    scopes.some((s) => s.startsWith(scope)),
    `${JSON.stringify(text)} in ${JSON.stringify(line)} has ${scopes.join(" ")}, expected ${scope}`
  );
}

const cases = [
  ["kalupu(a: ankam ni, b: ankam tho) -> ankam ante:", "kalupu", "entity.name.function.ante"],
  ["kalupu(a: ankam ni, b: ankam tho) -> ankam ante:", "ankam", "support.type.builtin.ante"],
  ["kalupu(a: ankam ni, b: ankam tho) -> ankam ante:", "ni", "keyword.operator.case-marker.ante"],
  ["kalupu(a: ankam ni, b: ankam tho) -> ankam ante:", "->", "keyword.operator.arrow.ante"],
  ["kalupu(a: ankam ni, b: ankam tho) -> ankam ante:", "ante", "keyword.other.declaration.ante"],
  ["andhariki choopu(r: Rahasyam) -> khali ante:", "andhariki", "storage.modifier.ante"],
  ["    10 ni, 20 tho kalupu() |> choopinchu()", "10", "constant.numeric.integer.ante"],
  ["    10 ni, 20 tho kalupu() |> choopinchu()", "tho", "keyword.operator.case-marker.ante"],
  ["    10 ni, 20 tho kalupu() |> choopinchu()", "kalupu", "entity.name.function.call.ante"],
  ["    10 ni, 20 tho kalupu() |> choopinchu()", "choopinchu", "support.function.builtin.ante"],
  ["    10 ni, 20 tho kalupu() |> choopinchu()", "|>", "keyword.operator.pipe.ante"],
  ["    y ante v ni maarchu[ankam64]()", "maarchu", "support.function.builtin.ante"],
  ["    x lo 5 pettu", "pettu", "keyword.operator.assignment.ante"],
  ["    x lo 5 pettu", "lo", "keyword.operator.case-marker.ante"],
  ["# vyakhya", "# vyakhya", "comment.line.number-sign.ante"],
  ["## doc comment", "## doc comment", "comment.line.documentation.ante"],
  ['    s ante f"{peru} ki {a + 1} ellu"', "f", "storage.type.string.ante"],
  ['    s ante f"{peru} ki {a + 1} ellu"', "peru", "meta.interpolation.ante"],
  ['    s ante f"{peru} ki {a + 1} ellu"', " ellu", "string.quoted.double.interpolated.ante"],
  ['    t ante "Telugu lo \\u{C24}\\n"', "\\u{C24}", "constant.character.escape.ante"],
  ['    t ante "Telugu lo \\u{C24}\\n"', "\\n", "constant.character.escape.ante"],
  ["    c ante 'త'", "'త'", "constant.character.ante"],
  ["@automatic Polchochu, Choopinchochu", "automatic", "entity.name.function.decorator.ante"],
  ["@automatic Polchochu, Choopinchochu", "Polchochu", "entity.name.type.ante"],
  ["    score >= 90 aithe:", "aithe", "keyword.control.ante"],
  ["    score >= 90 aithe:", ">=", "keyword.operator.comparison.ante"],
  ["    naa.balance += amount", "naa", "variable.language.ante"],
  ["    naa.balance += amount", "+=", "keyword.operator.assignment.compound.ante"],
  ["    o ante undhi(5)", "undhi", "support.function.constructor.ante"],
  ["Account ki Polchochu gunam undhi", "undhi", "keyword.other.declaration.ante"],
  ["Account ki Polchochu gunam undhi", "Account", "entity.name.type.ante"],
  ["    sare icchey", "sare", "constant.language.ante"],
  ["    sare icchey", "icchey", "keyword.control.ante"],
  ["    x ante 1_000 + 0xFF + 1.5e-3", "1_000", "constant.numeric.integer.ante"],
  ["    x ante 1_000 + 0xFF + 1.5e-3", "0xFF", "constant.numeric.hex.ante"],
  ["    x ante 1_000 + 0xFF + 1.5e-3", "1.5e-3", "constant.numeric.float.ante"],
  ["    0 nunchi n lopu okko i ki:", "nunchi", "keyword.operator.case-marker.ante"],
  ["    0 nunchi n lopu okko i ki:", "okko", "keyword.control.ante"],
  ["    p ante x @ w", "@", "keyword.operator.arithmetic.ante"],
  ["    async", "async", "invalid.deprecated.reserved.ante"],
];

/** Keywords as declared in the compiler's keyword table. */
function compilerKeywords() {
  const source = fs.readFileSync(path.join(root, "../../antec/src/token.rs"), "utf8");
  const start = source.indexOf("keywords! {");
  const table = source.slice(start, source.indexOf("impl Keyword {", start));
  return [...table.matchAll(/=\s*"([a-z0-9_]+)"/g)].map((m) => m[1]);
}

(async () => {
  const grammar = await loadGrammar();
  let failures = 0;
  for (const [line, text, scope] of cases) {
    try {
      expectScope(grammar, line, text, scope);
    } catch (err) {
      failures++;
      console.error(`✗ ${err.message}`);
    }
  }
  const keywords = compilerKeywords();
  assert.ok(keywords.length > 50, `expected the keyword table, found ${keywords.length} keywords`);
  for (const kw of keywords) {
    const tokens = tokenize(grammar, `    ${kw}`).filter((t) => t.text === kw);
    if (!tokens.length || tokens[0].scopes.length < 2) {
      failures++;
      console.error(`✗ keyword \`${kw}\` isn't highlighted`);
    }
  }
  if (failures) {
    console.error(`\n${failures} grammar check(s) failed`);
    process.exit(1);
  }
  console.log(`✅ grammar: ${cases.length} scope checks + ${keywords.length} compiler keywords, anthaa ok!`);
})();
