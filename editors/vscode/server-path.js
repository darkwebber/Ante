// Finds the `antec` executable for the language server. Plain Node, no
// `vscode`, so the tests can run it.

const fs = require("fs");
const os = require("os");
const path = require("path");

function isFile(p) {
  try {
    return fs.statSync(p).isFile();
  } catch {
    return false;
  }
}

/**
 * Where `antec` is, or `null`.
 *
 * `setting` is `ante.antecPath`: a bare name looked up on `PATH` (and in
 * `~/.cargo/bin`, which an editor started from the Dock may not have on its
 * `PATH`), or a path, relative ones against `base` (the workspace folder).
 */
function findAntec(setting, { env = process.env, platform = process.platform, base } = {}) {
  const name = (setting || "antec").trim();
  const exts = platform === "win32" ? (env.PATHEXT || ".EXE;.CMD;.BAT").split(";").filter(Boolean) : [];
  const candidates = (p) => [p, ...exts.map((e) => p + e.toLowerCase()), ...exts.map((e) => p + e)];
  const hasSep = name.includes("/") || name.includes("\\");
  if (hasSep || path.isAbsolute(name)) {
    let p = name.startsWith("~/") ? path.join(env.HOME || os.homedir(), name.slice(2)) : name;
    if (!path.isAbsolute(p)) {
      if (!base) return null;
      p = path.resolve(base, p);
    }
    return candidates(p).find(isFile) || null;
  }
  const sep = platform === "win32" ? ";" : ":";
  const dirs = (env.PATH || env.Path || "").split(sep).filter(Boolean);
  const home = env.HOME || env.USERPROFILE;
  if (home) dirs.push(path.join(home, ".cargo", "bin"));
  for (const dir of dirs) {
    const found = candidates(path.join(dir, name)).find(isFile);
    if (found) return found;
  }
  return null;
}

module.exports = { findAntec };
