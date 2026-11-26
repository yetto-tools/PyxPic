"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const Module = require("node:module");
const { EventEmitter } = require("node:events");
const original = Module._load;
Module._load = function(request, parent, main) {
  if (request === "vscode") return { Uri: { joinPath: (...parts) => parts.join("/") } };
  return original.call(this, request, parent, main);
};
const serial = require("../serial");
Module._load = original;

const webview = { cspSource: "vscode-webview:", asWebviewUri: (value) => value };
const tick = () => new Promise((resolve) => setImmediate(resolve));

test("la lista de puertos se lee del JSON versionado de la CLI", () => {
  const ok = serial.parsePorts(JSON.stringify({ version: 1, ports: [{ name: "COM3", product: "CP210x" }, { name: "COM4" }] }));
  assert.deepEqual(ok.ports.map((p) => p.label), ["COM3 · CP210x", "COM4"]);
  assert.match(serial.parsePorts(JSON.stringify({ version: 9, ports: [] })).error, /versión 9/);
  assert.match(serial.parsePorts("basura").error, /no devolvió/);
});

test("los eventos de la CLI se interpretan y lo ilegible se muestra tal cual", () => {
  assert.deepEqual(serial.parseEvent(JSON.stringify({ version: 1, type: "data", t_ms: 1125, hex: "48 69", text: "Hi\\x0A" })), { kind: "data", text: "Hi\\x0A", hex: "48 69", t_ms: 1125 });
  assert.equal(serial.parseEvent(JSON.stringify({ version: 1, type: "closed", message: "se desconectó" })).kind, "error");
  assert.equal(serial.parseEvent(JSON.stringify({ version: 1, type: "open", message: "COM3" })).kind, "info");
  assert.match(serial.parseEvent(JSON.stringify({ version: 2, type: "data" })).text, /versión 2/);
  assert.deepEqual(serial.parseEvent("no es json"), { kind: "info", text: "no es json" });
});

test("las lineas se arman aunque lleguen a pedazos", () => {
  const got = [];
  const feed = serial.lineSplitter((l) => got.push(l));
  feed("uno\ndo");
  feed("s\r\ntres");
  assert.deepEqual(got, ["uno", "dos"]);
  feed("\n");
  assert.deepEqual(got, ["uno", "dos", "tres"]);
});

test("el formato muestra marcas de tiempo, hexadecimal y quita el fin de linea", () => {
  const item = { kind: "data", text: "ADC0 = 5\\x0D\\x0A", hex: "41 44", t_ms: 61_005 };
  assert.equal(serial.formatLine(item, { timestamps: true }), "[01:01.005] ADC0 = 5");
  assert.equal(serial.formatLine(item, { encoding: "hex" }), "41 44");
});

test("los argumentos de la CLI salen de las opciones de la vista", () => {
  assert.deepEqual(serial.buildArgs({ port: "COM3", baud: 115200, filter: "ADC", eol: "crlf", sendHex: true }), ["monitor", "--port", "COM3", "--baud", "115200", "--format", "jsonl", "--filter", "ADC", "--eol", "crlf", "--send-encoding", "hex"]);
  assert.deepEqual(serial.buildArgs({ sim: "a.bas", seconds: 3 }), ["monitor", "--sim", "a.bas", "--seconds", "3", "--format", "jsonl"]);
});

test("el HTML permite solo el script con nonce y escapa los nombres de puerto", () => {
  const out = serial.html(webview, "ext", { ports: [{ name: "COM\"<3", label: "<b>x</b>" }], baud: 9600 });
  const nonce = /script-src 'nonce-([^']+)'/.exec(out)[1];
  assert.match(nonce, /^[A-Za-z0-9+/=]+$/);
  assert.ok(out.includes(`<script nonce="${nonce}">`));
  assert.ok(!out.includes("<b>x</b>") && out.includes("&lt;b&gt;"));
});

function harness(deps) {
  const messages = [];
  let received;
  const view = { webview: { options: {}, html: "", postMessage: (m) => { messages.push(m); return Promise.resolve(true); }, onDidReceiveMessage: (f) => { received = f; }, cspSource: "x", asWebviewUri: (v) => v }, onDidDispose: () => {} };
  const vscode = { Uri: { joinPath: (...p) => p.join("/") }, window: { registerWebviewViewProvider: (_id, provider) => provider.resolveWebviewView(view) }, env: { clipboard: { writeText: () => {} } } };
  const api = serial.register(vscode, { extensionUri: "ext", subscriptions: [] }, deps);
  return { api, view, messages, send: (m) => received(m) };
}

function fakeProcess() {
  const p = new EventEmitter();
  p.stdout = new EventEmitter();
  p.stderr = new EventEmitter();
  p.stdin = { written: [], write(t) { this.written.push(t); } };
  p.killed = false;
  p.kill = () => { p.killed = true; };
  return p;
}

test("una sesion muestra los datos, envia por la entrada de la CLI y se puede detener", async () => {
  const proc = fakeProcess();
  const spawned = [];
  const h = harness({ listPorts: async () => ({ stdout: JSON.stringify({ version: 1, ports: [{ name: "COM3" }] }) }), spawn: (a) => { spawned.push(a); return proc; }, activeSource: () => undefined });
  await tick();
  h.send({ command: "start", options: { port: "COM3", baud: 9600, timestamps: false } });
  assert.deepEqual(spawned[0].slice(0, 5), ["monitor", "--port", "COM3", "--baud", "9600"]);
  proc.stdout.emit("data", `${JSON.stringify({ version: 1, type: "data", t_ms: 0, hex: "4F 4B", text: "OK" })}\n`);
  assert.ok(h.messages.some((m) => m.type === "line" && m.text === "OK"));
  h.send({ command: "send", text: "hola" });
  assert.deepEqual(proc.stdin.written, ["hola\n"]);
  h.send({ command: "stop" });
  assert.ok(proc.killed);
  assert.ok(h.messages.some((m) => m.type === "status" && m.running === false));
});

test("un puerto ocupado o un fallo de la CLI se muestra y deja la vista lista para reintentar", async () => {
  const proc = fakeProcess();
  const h = harness({ listPorts: async () => ({ stdout: JSON.stringify({ version: 1, ports: [] }) }), spawn: () => proc, activeSource: () => "a.bas" });
  await tick();
  h.send({ command: "start", options: { port: "COM3", baud: 9600 } });
  proc.stderr.emit("data", "el puerto 'COM3' esta ocupado\n");
  proc.emit("close", 2);
  assert.ok(h.messages.some((m) => m.kind === "error" && /ocupado/.test(m.text)));
  assert.ok(h.messages.some((m) => m.kind === "error" && /código 2/.test(m.text)));
  assert.equal(h.api.isRunning(), false);
  h.send({ command: "send", text: "x" });
  assert.ok(h.messages.some((m) => /pulsa Iniciar/.test(m.text || "")));
  h.send({ command: "start", options: { baud: 9600 } });
  assert.ok(h.messages.some((m) => /Elige un puerto/.test(m.text || "")));
});

test("la CLI ausente se explica al listar los puertos", async () => {
  const h = harness({ listPorts: async () => ({ error: new Error("spawn pypic ENOENT"), stdout: "" }), spawn: () => fakeProcess(), activeSource: () => undefined });
  await tick();
  assert.match(h.view.webview.html, /No se encontró la CLI de PyPIC/);
});
