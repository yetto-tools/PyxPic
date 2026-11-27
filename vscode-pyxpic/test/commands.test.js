"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const Module = require("node:module");
const { execFileSync } = require("node:child_process");

test("los comandos comprueban y compilan con la CLI real y muestran sus errores", async () => {
  const root = path.resolve(__dirname, "../..");
  const binary = path.join(root, "target", "debug", process.platform === "win32" ? "pyxpic.exe" : "pyxpic");
  assert.ok(fs.existsSync(binary), "ejecuta cargo build -p pyxpic-cli antes de las pruebas");
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "pyxpic extension "));
  const source = path.join(dir, "blink.py");
  const hex = path.join(dir, "blink.hex");
  fs.copyFileSync(path.join(root, "examples", "python", "gpio", "blink.py"), source);
  const commands = new Map();
  const sidebars = new Map();
  const webviews = new Map();
  const messages = [];
  let result;
  let output = "";
  let problems = [];
  let saved;
  let problemsUpdated;
  let selection;
  const updates = [];
  const completed = () => new Promise((resolve) => { result = resolve; });
  const vscode = {
    Uri: { file: (fsPath) => ({ fsPath }) },
    TreeItem: class { constructor(label) { this.label = label; } },
    TreeItemCollapsibleState: { None: 0 },
    ThemeIcon: class { constructor(id) { this.id = id; } },
    Range: class { constructor(...args) { this.args = args; } },
    Diagnostic: class { constructor(range, message, severity) { Object.assign(this, { range, message, severity }); } },
    DiagnosticSeverity: { Error: 0, Warning: 1 },
    Hover: class { constructor(contents) { this.contents = contents; } },
    CompletionItem: class { constructor(label) { this.label = label; } },
    ConfigurationTarget: { WorkspaceFolder: 3, Global: 1 },
    StatusBarAlignment: { Left: 1 },
    languages: {
      createDiagnosticCollection: () => ({ set(_uri, items) { problems = items; if (problemsUpdated) problemsUpdated(); }, delete() {}, dispose() {} }),
      registerHoverProvider: () => ({ dispose() {} }),
      registerCompletionItemProvider: () => ({ dispose() {} }),
    },
    window: {
      registerTreeDataProvider: (id, provider) => { sidebars.set(id, provider); return { dispose() {} }; },
      registerWebviewViewProvider: (id, provider) => { webviews.set(id, provider); return { dispose() {} }; },
      createStatusBarItem: () => ({ show() {}, hide() {}, dispose() {} }),
      onDidChangeActiveTextEditor: () => ({ dispose() {} }),
      activeTextEditor: { document: { uri: { scheme: "file", fsPath: source }, languageId: "python", isDirty: false } },
      createOutputChannel: () => ({ clear() { output = ""; }, append(s) { output += s; }, appendLine(s) { output += `${s}\n`; }, show() {}, dispose() {} }),
      showSaveDialog: async () => ({ fsPath: hex }),
      showOpenDialog: async () => selection,
      showErrorMessage: (s) => { messages.push(s); result(); },
      showInformationMessage: (s) => { messages.push(s); result(); },
      showWarningMessage: async (s, _options, accept) => { messages.push(s); return accept; },
    },
    workspace: {
      isTrusted: true,
      textDocuments: [],
      onDidOpenTextDocument: () => ({ dispose() {} }),
      onDidChangeTextDocument: () => ({ dispose() {} }),
      onDidCloseTextDocument: () => ({ dispose() {} }),
      onDidSaveTextDocument: (handler) => { saved = handler; return { dispose() {} }; },
      getConfiguration: () => ({ get: (key) => key === "executable" ? binary : path.join(root, "devices"), update: async (...args) => { updates.push(args); } }),
      getWorkspaceFolder: () => ({ uri: { fsPath: root } }),
    },
    commands: { registerCommand: (name, fn) => { commands.set(name, fn); return { dispose() {} }; } },
  };
  const original = Module._load;
  Module._load = function(request, parent, main) { return request === "vscode" ? vscode : original.call(this, request, parent, main); };
  try {
    const state = new Map();
    require("../extension").activate({ subscriptions: [], workspaceState: { get: (key) => state.get(key), update: async (key, value) => { state.set(key, value); } } });
  } finally {
    Module._load = original;
  }
  const actionTree = sidebars.get("pyxpic.actions");
  const sections = actionTree.getChildren();
  assert.deepEqual(sections.map((section) => section.label), ["Proyecto", "Construir", "Entorno"]);
  assert.deepEqual(actionTree.getChildren(sections[0]).map((action) => action.command), ["pyxpic.welcome", "pyxpic.newProject", "pyxpic.openProject", "pyxpic.openFile", "pyxpic.examples"]);
  assert.deepEqual(actionTree.getChildren(sections[1]).map((action) => action.command), ["pyxpic.check", "pyxpic.build", "pyxpic.clean", "pyxpic.selectTarget", "pyxpic.projectCheck", "pyxpic.status"]);
  assert.deepEqual(actionTree.getChildren(sections[2]).map((action) => action.command), ["pyxpic.selectCompiler", "pyxpic.selectDevices", "pyxpic.selectPicAs", "pyxpic.settings"]);
  assert.ok(webviews.has("pyxpic.pinout"));
  assert.ok(webviews.has("pyxpic.resources"));
  let done = completed();
  await commands.get("pyxpic.check")();
  await done;
  assert.match(messages.pop(), /comprobación terminada/);
  done = completed();
  await commands.get("pyxpic.build")();
  await done;
  assert.match(messages.pop(), /compilación terminada/);
  assert.match(fs.readFileSync(hex, "utf8"), /^:/);
  // limpiar borra el .hex que la compilación dijo haber generado, y solo ese
  const bystander = path.join(path.dirname(hex), "ajeno.hex");
  fs.writeFileSync(bystander, ":00000001FF" + String.fromCharCode(10));
  await new Promise((resolve) => setTimeout(resolve, 50));
  await commands.get("pyxpic.clean")();
  assert.ok(!fs.existsSync(hex), "el .hex generado se borra");
  assert.ok(fs.existsSync(bystander), "un .hex que PyxPic no generó no se toca");
  fs.rmSync(bystander);
  assert.match(messages.pop(), /limpieza terminada/);
  done = completed();
  await commands.get("pyxpic.clean")();
  await done;
  assert.match(messages.pop(), /no hay archivos \.hex/);
  fs.writeFileSync(source, "def main(\n");
  done = completed();
  await commands.get("pyxpic.check")();
  await done;
  assert.match(messages.pop(), /no pudo completar/);
  assert.match(output, /error\[PYP/);
  assert.ok(problems.some((problem) => problem.severity === 0 && String(problem.code).startsWith("PYP")));
  vscode.window.activeTextEditor.document.languageId = "pyxpic";
  const checked = new Promise((resolve) => { problemsUpdated = resolve; });
  saved(vscode.window.activeTextEditor.document);
  await checked;
  assert.ok(problems.some((problem) => problem.severity === 0));
  selection = undefined;
  await commands.get("pyxpic.selectCompiler")();
  assert.deepEqual(updates, []);
  selection = [{ fsPath: binary }];
  await commands.get("pyxpic.selectCompiler")();
  assert.deepEqual(updates[0], ["executable", binary, 3]);
  selection = [{ fsPath: path.join(root, "devices") }];
  await commands.get("pyxpic.selectDevices")();
  assert.deepEqual(updates[1], ["devicesDir", path.join(root, "devices"), 3]);
});

test("identifica PyxPic y conserva ubicación y severidad de diagnósticos", () => {
  const language = require("../language");
  assert.equal(language.isPyxPic("from pyxpic.periph.adc import ADC\n"), true);
  assert.equal(language.isPyxPic("from math import sin\n"), false);
  assert.deepEqual(language.diagnostics("C:\\x.py:3:4: warning[PYP3001]: aviso", "C:\\x.py"), [
    { line: 2, column: 3, severity: "warning", code: "PYP3001", message: "aviso" },
  ]);
  assert.match(language.info.uint16, /16 bits/);
  for (const name of ["CLOCK", "FREQUENCY", "OSC_MODE", "Internal", "Crystal", "External", "SOURCE_FREQ", "MHz", "kHz", "Hz"]) {
    assert.ok(language.completions("").includes(name));
    assert.ok(language.info[name]);
  }
  assert.ok(language.completions("UART.").includes("write_text"));
  assert.ok(language.completions("ADC.re").includes("read"));
  assert.ok(!language.completions("ADC.").includes("write_text"));
  assert.deepEqual(language.completions("PWM1."), ["start", "duty", "stop"]);
  assert.deepEqual(language.completions("EEPROM."), ["read", "write", "start_write", "busy", "failed"]);
  assert.ok(language.info.EEPROM);
  assert.ok(language.info.PWM1 && language.info.PWM2 && language.info.duty);
  assert.deepEqual(language.completions("otro."), []);
  const root = path.resolve(__dirname, "../..");
  const tools = language.projectTools(path.join(root, "examples", "python", "uart", "adc_serial.py"), path.dirname(root), "pyxpic", "");
  assert.equal(tools.executable, path.join(root, "target", "debug", process.platform === "win32" ? "pyxpic.exe" : "pyxpic"));
  assert.equal(tools.devices, path.join(root, "devices"));
  const workspaceTools = language.projectTools(root, root, "pyxpic", "");
  assert.equal(workspaceTools.executable, path.join(root, "target", "debug", process.platform === "win32" ? "pyxpic.exe" : "pyxpic"));
  assert.equal(workspaceTools.devices, path.join(root, "devices"));
  assert.equal(language.projectTools(path.join(root, "examples", "python", "gpio", "blink.py"), root, "custom-compiler", "custom-devices").executable, "custom-compiler");
});

test("el resumen visible mantiene la versión del manifiesto", () => {
  const manifest = require("../package.json");
  assert.match(manifest.description, new RegExp(`^v${manifest.version.replaceAll(".", "\\.")} · `));
});

test("la plantilla de proyecto para cristal compila con la CLI", () => {
  const project = require("../project");
  const source = project.source("PIC16F887", "Crystal 20MHz");
  const root = path.resolve(__dirname, "../..");
  const binary = path.join(root, "target", "debug", process.platform === "win32" ? "pyxpic.exe" : "pyxpic");
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "pyxpic-project-template-"));
  const main = path.join(dir, "main.py");
  fs.writeFileSync(main, source);
  const result = execFileSync(binary, ["check", main, "--devices-dir", path.join(root, "devices")], { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
  assert.match(result, /main\.py: ok/);
});

test("PyxPic BASIC (.bas): lenguaje, gramatica, ayuda, completado y dispositivo declarado", () => {
  const manifest = require("../package.json");
  const basic = manifest.contributes.languages.find((l) => l.id === "pyxpic-basic");
  assert.deepEqual(basic.extensions, [".bas"]);
  assert.ok(fs.existsSync(path.join(__dirname, "..", basic.configuration.slice(2))), "existe la configuracion del lenguaje");
  const grammar = manifest.contributes.grammars.find((g) => g.language === "pyxpic-basic");
  const syntax = JSON.parse(fs.readFileSync(path.join(__dirname, "..", grammar.path), "utf8"));
  assert.equal(syntax.scopeName, grammar.scopeName);
  assert.ok(manifest.activationEvents.includes("onLanguage:pyxpic-basic"));
  assert.ok(manifest.contributes.menus["editor/title"].every((m) => m.when.includes("pyxpic-basic")), "check, build y clean tambien en .bas");
  assert.ok(manifest.files.includes("language-configuration-basic.json"));
  const language = require("../language");
  assert.equal(language.declaredDevice("' parpadeo\nDEVICE PIC16F887   ' el PIC\nCLOCK INTERNAL 4 MHZ\n"), "PIC16F887");
  assert.equal(language.declaredDevice("device pic16f887\n"), "PIC16F887");
  assert.equal(language.declaredDevice("from pyxpic.device import PIC16F887\n"), "PIC16F887", "Python sigue igual");
  assert.ok(language.basicCompletions("HI").includes("HIGH") && language.basicCompletions("").includes("READADC10"));
  assert.deepEqual(language.basicCompletions("EEPROM."), ["read", "write", "start_write", "busy", "failed"]);
  assert.deepEqual(language.basicCompletions("adc.re"), language.basicCompletions("ADC."), "el objeto no distingue mayusculas");
  assert.deepEqual(language.basicCompletions("x = b0."), []);
  for (const word of ["device", "clock", "baud", "symbol", "dim", "high", "pause", "readadc10", "read", "write", "pwmout", "sertxd", "serrxd"]) assert.ok(language.basicInfo[word], word);
  const d = language.diagnostics("t.bas:3:6: error[PYB0040]: 300 no cabe en un BYTE (0 a 255)\n", "t.bas");
  assert.deepEqual(d, [{ line: 2, column: 5, severity: "error", code: "PYB0040", message: "300 no cabe en un BYTE (0 a 255)" }]);
});
