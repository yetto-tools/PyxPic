"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const Module = require("node:module");
const original = Module._load;
Module._load = function(request, parent, main) {
  if (request === "vscode") return { Uri: { joinPath: (...parts) => parts.join("/") } };
  return original.call(this, request, parent, main);
};
const pinout = require("../pinout");
Module._load = original;

test("carga el PDIP-40 desde el Device Pack", () => {
  const devices = path.resolve(__dirname, "../../devices");
  const model = pinout.readDevice(devices, "PIC16F887");
  assert.equal(model.kind, "ready");
  assert.equal(model.packages[0].name, "PDIP40");
  assert.equal(model.packages[0].pins.length, 40);
  assert.deepEqual(model.packages[0].pins.find((pin) => pin.number === 19), { number: 19, name: "RD0", port: "D", bit: 0, functions: [] });
  assert.deepEqual(pinout.catalogDevices(devices).map((entry) => entry.name), ["PIC16F628A", "PIC16F887"]);
  const small = pinout.readDevice(devices, "PIC16F628A");
  assert.equal(small.packages[0].name, "PDIP18");
  assert.equal(small.packages[0].pins.find((pin) => pin.number === 6).name, "RB0");
});

test("renderiza la orientación física y funciones accesibles", () => {
  const catalog = pinout.catalogDevices(path.resolve(__dirname, "../../devices"));
  const model = pinout.resolveModel({ catalog, detected: { name: "PIC16F887", source: "Detectado en el archivo activo" } });
  const output = pinout.html({ cspSource: "vscode-webview:", asWebviewUri: (value) => value }, "extension", model);
  assert.match(output, /PIC16F887/);
  assert.match(output, /Detectado en el archivo activo/);
  assert.match(output, /Visualizar PIC/);
  assert.match(output, /Automático · PIC16F887/);
  assert.match(output, /Encapsulado/);
  assert.match(output, /PDIP40 · 40 pines/);
  assert.match(output, /title="Pin 2 · PORTA · RA0 · Tipo: Digital y analógico · Funciones: AN0, ULPWU, C12IN0-"/);
  assert.match(output, /aria-label="Pin 2: PORTA, RA0 \/ AN0 \/ ULPWU \/ C12IN0-"/);
  assert.ok(output.indexOf("Pin 40: PORTB, RB7") < output.indexOf("Pin 21: PORTD, RD2"));
  assert.match(output, /Fuente del Device Pack: DS40001291H p\.7/);
});

test("ofrece el catálogo y el modo automático cuando no se detecta un PIC", () => {
  const catalog = pinout.catalogDevices(path.resolve(__dirname, "../../devices"));
  const model = pinout.resolveModel({ catalog });
  const output = pinout.html({ cspSource: "vscode-webview:", asWebviewUri: (value) => value }, "extension", model);
  assert.equal(model.selectionMode, "automatic");
  assert.match(output, /No se detectó un PIC/);
  assert.match(output, /Visualizar PIC/);
  assert.match(output, /PIC16F887 · PIC16 mid-range/);
});

test("permite inspeccionar manualmente otro PIC sin perder la detección automática", () => {
  const base = pinout.catalogDevices(path.resolve(__dirname, "../../devices"))[0];
  const alternate = { ...base, name: "PIC12F625", model: { ...base.model, device: { ...base.model.device, name: "PIC12F625" } } };
  const model = pinout.resolveModel({
    catalog: [base, alternate],
    detected: { name: "PIC16F887", source: "Detectado en el archivo activo" },
    selectedName: "PIC12F625",
  });
  const output = pinout.html({ cspSource: "vscode-webview:", asWebviewUri: (value) => value }, "extension", model);
  assert.equal(model.device.name, "PIC12F625");
  assert.equal(model.selectionMode, "manual");
  assert.match(output, /<option value="PIC12F625" selected>/);
  assert.match(output, /Automático · PIC16F887/);
  assert.match(output, /no modifica el proyecto/);
});

test("prioriza el PIC del fuente y después el perfil activo del proyecto", () => {
  const report = {
    defaultTarget: "release",
    targets: [
      { name: "debug", device: "PIC12F625" },
      { name: "release", device: "PIC16F887" },
    ],
  };
  assert.deepEqual(pinout.detectedDevice("PIC18F4550", report, "debug"), { name: "PIC18F4550", source: "Detectado en el archivo activo" });
  assert.deepEqual(pinout.detectedDevice(undefined, report, "debug"), { name: "PIC12F625", source: "Perfil de proyecto: debug" });
  assert.deepEqual(pinout.detectedDevice(undefined, report), { name: "PIC16F887", source: "Perfil de proyecto: release" });
});

test("explica cómo recuperar un Device Pack ausente", () => {
  assert.deepEqual(pinout.readDevice("", "PIC16F887"), { kind: "error", message: "Configura la carpeta de Device Packs para mostrar el pinout." });
});

test("rechaza un encapsulado incompleto en el límite del Device Pack", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "pyxpic-pinout-"));
  const device = path.join(root, "pic16f887");
  fs.mkdirSync(device);
  fs.writeFileSync(path.join(device, "device.json"), JSON.stringify({ name: "PIC16F887" }));
  fs.writeFileSync(path.join(device, "pins.json"), JSON.stringify({ packages: [{ name: "PDIP40", pin_count: 40, pins: [] }] }));
  const model = pinout.readDevice(root, "PIC16F887");
  assert.deepEqual(model, { kind: "error", message: "El Device Pack de PIC16F887 no contiene encapsulados válidos." });
});

test("el nonce de la CSP es base64 válido: con un punto el script se bloquea y los clics no hacen nada", () => {
  const catalog = pinout.catalogDevices(path.resolve(__dirname, "../../devices"));
  const model = pinout.resolveModel({ catalog, detected: { name: "PIC16F887", source: "x" } });
  const output = pinout.html({ cspSource: "vscode-webview:", asWebviewUri: (value) => value }, "extension", model);
  const nonce = /script-src 'nonce-([^']+)'/.exec(output)[1];
  assert.match(nonce, /^[A-Za-z0-9+/]+=*$/);
  assert.ok(output.includes(`<script nonce="${nonce}">`));
});

test("muestra límites eléctricos, pull-up y registros desde el Device Pack, no desde una tabla propia", () => {
  const catalog = pinout.catalogDevices(path.resolve(__dirname, "../../devices"));
  const model = pinout.resolveModel({ catalog, detected: { name: "PIC16F887", source: "x" } });
  const output = pinout.html({ cspSource: "vscode-webview:", asWebviewUri: (value) => value }, "extension", model);
  const pin = (number) => new RegExp(`data-number="${number}"[^>]*data-registers="([^"]*)" data-current="([^"]*)" data-levels="([^"]*)" data-pullup="([^"]*)" data-snippet="([^"]*)"`).exec(output).slice(1);
  const [registers, current, levels, pullup, snippet] = pin(33); // RB0
  assert.match(registers, /WPUB&lt;0&gt;.*IOCB&lt;0&gt;/);
  assert.match(current, /25 mA/);
  assert.match(levels, /0,6 V a 8,5 mA/);
  assert.match(pullup, /^Sí: WPUB&lt;0&gt; = 1 con RBPU = 0 · 50 a 400 µA/);
  assert.equal(snippet, "from pyxpic.periph.gpio import RB0");
  assert.match(pin(19)[3], /^Sin pull-up interno/); // RD0
  assert.match(pin(12)[1], /95 mA saliendo por VSS/);
  assert.equal(pin(12)[3], "");
  assert.match(output, /id="detail-pullup-row" hidden/); // el pin 1 (MCLR) abre la vista sin pull-up
});
