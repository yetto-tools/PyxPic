"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const { execFileSync } = require("node:child_process");
const original = Module._load;
Module._load = function(request, parent, main) {
  if (request === "vscode") return { Uri: { joinPath: (...parts) => parts.join("/") } };
  return original.call(this, request, parent, main);
};
const variables = require("../variables");
Module._load = original;

const root = path.resolve(__dirname, "../..");
const binary = path.join(root, "target", "debug", process.platform === "win32" ? "pyxpic.exe" : "pyxpic");
const webview = { cspSource: "vscode-webview:", asWebviewUri: (value) => value };

function report(overrides = {}) {
  const predefined = [
    ...Array.from({ length: 28 }, (_, i) => ({ name: `b${i}`, kind: "predefined", type: "BYTE", length: null, line: null, used: i === 0, detail: "" })),
    ...Array.from({ length: 14 }, (_, i) => ({ name: `w${i}`, kind: "predefined", type: "WORD", length: null, line: null, used: false, detail: "" })),
  ];
  return {
    version: 1, language: "basic", device: "PIC16F887",
    variables: [
      { name: "contador", kind: "dim", type: "BYTE", length: null, line: 4, used: true, detail: "" },
      { name: "buf", kind: "dim", type: "BYTE", length: 16, line: 5, used: false, detail: "" },
      { name: "led", kind: "symbol", type: "-", length: null, line: 6, used: true, detail: "RB0" },
      ...predefined,
    ],
    ...overrides,
  };
}

test("la lista de variables la da la CLI y se rechaza lo que no se entiende", () => {
  assert.equal(variables.parseVariablesReport(JSON.stringify(report())).ok, true);
  assert.match(variables.parseVariablesReport("no es json").error, /no devolvió la lista/);
  assert.match(variables.parseVariablesReport(JSON.stringify({ variables: [] })).error, /no dice su versión/);
  assert.match(variables.parseVariablesReport(JSON.stringify(report({ version: 2 }))).error, /versión 2/);
  assert.match(variables.parseVariablesReport(JSON.stringify({ version: 1 })).error, /incompleto/);
});

test("separa lo declarado, lo predefinido en uso y lo disponible", () => {
  const g = variables.groupVariables(report());
  assert.deepEqual(g.declared.map((v) => v.name), ["contador", "buf", "led"]);
  assert.deepEqual(g.usedPredefined.map((v) => v.name), ["b0"]);
  assert.equal(g.freePredefined.length, 41);
});

test("la vista muestra las declaradas con su tipo, linea y uso, y las predefinidas en uso", () => {
  const out = variables.html(webview, "extension", { kind: "ready", report: report(), file: "led.bas", stale: false });
  assert.match(out, /<h1>Variables<\/h1><p>led\.bas · PIC16F887<\/p>/);
  assert.match(out, /contador<\/a><\/th><td>BYTE<\/td><td>4<\/td><td><span class="on">usada<\/span>/);
  assert.match(out, /buf<\/a>\(16\)<\/th>/, "un arreglo muestra su tamaño");
  assert.match(out, /led<\/a><\/th><td>alias<\/td><td>6<\/td><td><span class="on">usada<\/span><\/td><td>RB0<\/td>/);
  assert.match(out, /Predefinidas en uso<\/h2><span>1 \/ 42<\/span>/);
  assert.match(out, /Disponibles sin usar \(41\)/);
  assert.match(out, /command:pyxpic\.revealLine\?%5B4%5D/, "el nombre lleva a su linea");
  assert.match(out, /Content-Security-Policy[^>]*default-src 'none'/);
});

test("lo escrito por el usuario se escapa y los estados vacios explican que hacer", () => {
  const evil = report({ variables: [{ name: "<img src=x>", kind: "dim", type: "BYTE", length: null, line: 1, used: false, detail: "\"><script>" }] });
  const out = variables.html(webview, "extension", { kind: "ready", report: evil, file: "<b>.bas", stale: false });
  assert.doesNotMatch(out, /<img src=x>|<script>|<b>\.bas/);
  assert.match(variables.html(webview, "extension", { kind: "empty", message: "Abre un archivo PyxPic BASIC (.bas) para ver sus variables." }), /Abre un archivo PyxPic BASIC/);
  assert.match(variables.html(webview, "extension", { kind: "error", message: "boom" }), /role="alert">boom/);
  assert.match(variables.failureMessage({ error: Object.assign(new Error("x"), { code: "ENOENT" }) }), /No se encontró la CLI/);
});

test("el manifiesto declara la vista Variables solo para PyxPic BASIC y empaqueta su archivo", () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, "../package.json"), "utf8"));
  const view = manifest.contributes.views.pyxpic.find((v) => v.id === "pyxpic.variables");
  assert.equal(view.type, "webview");
  assert.equal(view.when, "pyxpic.basicActive");
  assert.ok(manifest.files.includes("variables.js") && manifest.files.includes("media"));
  assert.ok(manifest.activationEvents.includes("onView:pyxpic.variables"));
  assert.ok(manifest.contributes.commands.some((c) => c.command === "pyxpic.revealLine"));
});

test("la CLI real lista las variables de un programa BASIC con el texto por la entrada estandar", () => {
  assert.ok(fs.existsSync(binary), "ejecuta cargo build -p pyxpic-cli antes de las pruebas");
  const program = "DEVICE PIC16F887\nDIM contador AS BYTE\nSYMBOL led = RB0\nmain:\n  contador = contador + 1\n  HIGH led\n  b3 = 7\n  GOTO main\n";
  const out = execFileSync(binary, ["vars", "-", "--format", "json"], { input: program, encoding: "utf8" });
  const parsed = variables.parseVariablesReport(out);
  assert.equal(parsed.ok, true);
  const byName = Object.fromEntries(parsed.report.variables.map((v) => [v.name, v]));
  assert.deepEqual([byName.contador.kind, byName.contador.line, byName.contador.used], ["dim", 2, true]);
  assert.deepEqual([byName.led.kind, byName.led.detail, byName.led.used], ["symbol", "RB0", true]);
  assert.equal(byName.b3.used, true);
  assert.equal(byName.b4.used, false);
  assert.equal(parsed.report.device, "PIC16F887");
  // un archivo que no es BASIC se rechaza con un mensaje
  assert.throws(() => execFileSync(binary, ["vars", path.join(root, "examples", "python", "gpio", "blink.py")], { stdio: "pipe" }), /./);
});
