// Corrector ortográfico: si está Code Spell Checker, le da los diccionarios del modo "pyxpic" (Python y la API de PyxPic).
// Sin esa extensión no hace nada: PyxPic no depende de ella.
const path = require("node:path");

const SPELL_CHECKER = "streetsidesoftware.code-spell-checker";

async function registerSpelling(vscode, extensionPath, log = () => {}) {
  const checker = vscode.extensions?.getExtension(SPELL_CHECKER);
  if (!checker) return false;
  try {
    const api = await checker.activate();
    if (typeof api?.registerConfig !== "function") return false;
    api.registerConfig(path.join(extensionPath, "cspell", "cspell-ext.json"));
    return true;
  } catch (error) {
    log(`No se pudo configurar el corrector ortográfico: ${error.message}`);
    return false;
  }
}

module.exports = { registerSpelling, SPELL_CHECKER };
