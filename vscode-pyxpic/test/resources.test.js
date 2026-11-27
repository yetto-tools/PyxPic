"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const path = require("node:path");
const Module = require("node:module");
const original = Module._load;
Module._load = function(request, parent, main) {
  if (request === "vscode") return { Uri: { joinPath: (...parts) => parts.join("/") } };
  return original.call(this, request, parent, main);
};
const resources = require("../resources");
Module._load = original;

const devices = path.resolve(__dirname, "../../devices");
const report = {
  version: 1, tool: "pyxpic memory", file: "blink.py", device: "PIC16F887", complete: true, notes: [],
  program: { unit: "words", word_bits: 14, capacity: 8192, used: 27, free: 8165, used_percent: 0.33, highest_address_used: 30, startup_and_vectors: 7, config_words: 2 },
  ram: { unit: "bytes", chip_total: 368, compiler_usable: 80, used: 5, globals: 0, locals_and_workspace: 5, free: 75, used_percent: 6.25, reserved_common: 16, banks: [{ bank: 0, gpr_bytes: 96, compiler_capacity: 80, used: 5 }, { bank: 1, gpr_bytes: 80, compiler_capacity: 0, used: 0 }] },
  stack: { levels: 8, worst_case: 1, main: 1, interrupt: 0 },
  peripherals: ["ADC", "UART"], pins: ["RD0"],
};
const webview = { cspSource: "vscode-webview:", asWebviewUri: (value) => value };
const render = (model) => resources.html(webview, "extension", model);

test("el informe de la CLI es la unica fuente de los numeros medidos", () => {
  const parsed = resources.parseMemoryReport(JSON.stringify(report));
  assert.equal(parsed.ok, true);
  const model = resources.readResources(devices, "PIC16F887", "", { report: parsed.report });
  assert.equal(model.programAvailable, 8192);
  assert.equal(model.compilerRamAvailable, 80);
  assert.equal(model.eepromAvailable, 256);
  const out = render(model);
  assert.match(out, /27 \/ 8192 palabras de 14 bits · 0\.3%/);
  assert.match(out, /RAM del compilador<\/strong><span>5 \/ 80 bytes/);
  assert.match(out, /Pila de hardware<\/strong><span>1 \/ 8 niveles/);
  assert.match(out, /Dirección más alta de programa<\/dt><dd>0x001D/);
  assert.match(out, /<th scope="row">Banco 1<\/th><td>0 \/ 0<\/td><td>80<\/td>/);
  assert.match(out, /<span>ADC<\/span><span>UART<\/span>/);
  assert.match(out, /<span>RD0<\/span>/);
  assert.match(out, /medidos por la CLI/);
  assert.doesNotMatch(out, /notice/, "un informe completo y al dia no muestra avisos");
});

test("sin medicion muestra las capacidades del Device Pack y ofrece medir", () => {
  const model = resources.readResources(devices, "PIC16F887", "from pyxpic.periph.adc import ADC\nfrom pyxpic.periph.uart import UART\n", undefined);
  assert.deepEqual(model.usedPeripherals, ["ADC", "EUSART"]);
  const out = render(model);
  assert.match(out, /Todavía no se midió/);
  assert.match(out, /href="command:pyxpic\.refreshResources"/);
  assert.doesNotMatch(out, /<progress/, "sin medicion no hay barras inventadas");
});

test("un resultado parcial lo dice con sus notas", () => {
  const partial = { ...report, complete: false, notes: ["1 funcion(es) en ensamblador (swap): su uso de la pila no se analiza"] };
  const out = render(resources.readResources(devices, "PIC16F887", "", { report: partial }));
  assert.match(out, /Resultado parcial/);
  assert.match(out, /swap\): su uso de la pila no se analiza/);
});

test("lo desactualizado se marca y un fallo conserva la ultima medicion correcta", () => {
  const stale = render(resources.readResources(devices, "PIC16F887", "", { report, stale: true }));
  assert.match(stale, /Desactualizado/);
  const failed = render(resources.readResources(devices, "PIC16F887", "", { report, stale: true, error: "b.py:3:1: error[PYB0040]: 300 no cabe" }));
  assert.match(failed, /No se pudo medir/);
  assert.match(failed, /PYB0040/);
  assert.match(failed, /última medición correcta/);
  assert.match(failed, /27 \/ 8192/);
  const noReport = render(resources.readResources(devices, "PIC16F887", "", { error: "falta el dispositivo" }));
  assert.match(noReport, /No se pudo medir/);
  assert.doesNotMatch(noReport, /última medición/);
  assert.match(render(resources.readResources(devices, "PIC16F887", "", { measuring: true })), /Midiendo con la CLI/);
});

test("un informe de otro dispositivo no se atribuye a este", () => {
  const model = resources.readResources(devices, "PIC16F887", "", { report: { ...report, device: "PIC12F625" } });
  assert.equal(model.report, undefined);
});

test("entiende solo la version 1 del informe y explica cualquier otra cosa", () => {
  assert.equal(resources.parseMemoryReport(JSON.stringify({ ...report, version: 2 })).ok, false);
  assert.match(resources.parseMemoryReport(JSON.stringify({ ...report, version: 2 })).error, /versión 2/);
  assert.equal(resources.parseMemoryReport("no es json").ok, false);
  assert.equal(resources.parseMemoryReport(JSON.stringify({ version: 1, program: {} })).ok, false);
  assert.equal(resources.parseMemoryReport(JSON.stringify({ ...report, extra: "campo nuevo" })).ok, true, "agregar campos no rompe la lectura");
});

test("explica por que no se pudo medir: la CLI ausente o el error del compilador", () => {
  assert.match(resources.measureFailure({ error: Object.assign(new Error("spawn pyxpic ENOENT"), { code: "ENOENT" }), stdout: "", stderr: "" }), /No se encontró la CLI/);
  assert.match(resources.measureFailure({ error: new Error("exit 1"), stdout: "", stderr: "p.py:3:6: error[PYB0040]: 300 no cabe en un BYTE\n" }), /PYB0040/);
  assert.match(resources.measureFailure({ error: new Error("exit 1"), stdout: "", stderr: "" }), /no pudo medir/);
});
