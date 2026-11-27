"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync, spawnSync } = require("node:child_process");
const language = require("../language");

const root = path.resolve(__dirname, "../..");
const binary = path.join(root, "target", "debug", process.platform === "win32" ? "pyxpic.exe" : "pyxpic");
const devices = path.join(root, "devices");

/** El resultado de `pyxpic check - --lang ... --format json` con `text` por la entrada estándar (lo que hace el análisis en vivo). */
function live(text, lang = "bas") {
  assert.ok(fs.existsSync(binary), "ejecuta cargo build -p pyxpic-cli antes de las pruebas");
  const run = spawnSync(binary, ["check", "-", "--lang", lang, "--format", "json", "--devices-dir", devices], { input: text, encoding: "utf8" });
  return language.diagnosticsFromJson(run.stdout);
}

const LONG = "DEVICE PIC16F887\nCLOCK INTERNAL 8 MHZ\nTMR2.every_us 100003\nmain:\n  GOTO main\n";

test("el analisis en vivo distingue el analizador sintactico del semantico", () => {
  const syntax = live("DEVICE PIC16F887\nmain:\n  b0 = (\n");
  assert.ok(syntax.length && syntax.every((d) => d.phase === "syntax"), JSON.stringify(syntax));
  assert.equal(language.diagnosticSource(syntax[0]), "PyxPic · sintaxis");
  const semantic = live("DEVICE PIC16F887\nmain:\n  HIGH RE5\n  GOTO main\n");
  assert.ok(semantic.some((d) => d.phase === "semantic"), JSON.stringify(semantic));
  assert.equal(language.diagnosticSource(semantic.find((d) => d.phase === "semantic")), "PyxPic · semántica");
  assert.deepEqual(live("DEVICE PIC16F887\nCLOCK INTERNAL 4 MHZ\nmain:\n  GOTO main\n").filter((d) => d.severity === "error"), []);
});

test("un error de PyxPic llega con su causa y su solucion al panel Problemas", () => {
  const items = live(LONG);
  const d = items.find((x) => x.code === "PYP2126");
  assert.ok(d, JSON.stringify(items));
  assert.match(d.message, /Timer2 no puede dar un periodo exacto de 100\.003 ms/);
  assert.match(d.cause, /contador de 8 bits.*se desborda/);
  assert.match(d.fix, /100\.002 ms y 100\.004 ms/);
  const shown = language.describeDiagnostic(d);
  assert.match(shown, /^every_us\(100003\): .*\nCausa: .*\nSolución: /);
  // el rango cubre la sentencia, no un solo caracter
  assert.ok(d.endLine > d.line || d.endColumn > d.column + 1, JSON.stringify(d));
});

test("la salida de texto de la CLI tambien trae la causa y la solucion y la extension las une al diagnostico", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "pyxpic-diag-"));
  const file = path.join(dir, "largo.bas");
  fs.writeFileSync(file, LONG);
  let text = "";
  try {
    execFileSync(binary, ["check", file, "--devices-dir", devices], { encoding: "utf8", stdio: "pipe" });
  } catch (error) {
    text = `${error.stdout}\n${error.stderr}`;
  }
  const item = language.diagnostics(text, file).find((x) => x.code === "PYP2126");
  assert.ok(item && item.cause && item.fix, text);
  assert.match(item.cause, /se desborda/);
  // sin ese detalle (otros errores) el formato de siempre sigue funcionando
  assert.deepEqual(language.diagnostics(`${file}:3:5: error[PYB0015]: algo\n`, file), [{ line: 2, column: 4, severity: "error", code: "PYB0015", message: "algo" }]);
});

test("lo que no es un informe valido no se interpreta y el analisis en vivo se puede apagar", () => {
  assert.equal(language.diagnosticsFromJson("no es json"), undefined);
  assert.equal(language.diagnosticsFromJson(JSON.stringify({ version: 2, diagnostics: [] })), undefined);
  const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, "../package.json"), "utf8"));
  const setting = manifest.contributes.configuration.properties["pyxpic.liveCheck"];
  assert.equal(setting.default, true);
  assert.equal(setting.type, "boolean");
});
