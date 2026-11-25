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

test("lee capacidades y compara el resumen real de compilación", () => {
  const summary = resources.parseBuildSummary("Compilacion completada:\n  Dispositivo:   PIC16F887\n  Programa:      30 palabras\n  RAM:           7 bytes (banco 0)\n");
  assert.deepEqual(summary, { device: "PIC16F887", programUsed: 30, ramUsed: 7 });
  const model = resources.readResources(path.resolve(__dirname, "../../devices"), "PIC16F887", "from pypic.periph.adc import ADC\nfrom pypic.periph.uart import UART\n", summary);
  assert.equal(model.programAvailable, 8192);
  assert.equal(model.compilerRamAvailable, 80);
  assert.equal(model.ramAvailable, 368);
  assert.equal(model.eepromAvailable, 256);
  assert.deepEqual(model.usedPeripherals, ["ADC", "EUSART"]);
});

test("no atribuye el resumen de otro dispositivo", () => {
  const model = resources.readResources(path.resolve(__dirname, "../../devices"), "PIC16F887", "", { device: "PIC12F625", programUsed: 4, ramUsed: 1 });
  assert.equal(model.summary, undefined);
});
