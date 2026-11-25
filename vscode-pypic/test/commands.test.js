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
  const binary = path.join(root, "target", "debug", process.platform === "win32" ? "pypic.exe" : "pypic");
  assert.ok(fs.existsSync(binary), "ejecuta cargo build -p pypic-cli antes de las pruebas");
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "pypic extension "));
  const source = path.join(dir, "blink.py");
  const hex = path.join(dir, "blink.hex");
  fs.copyFileSync(path.join(root, "examples", "blink.py"), source);
  const commands = new Map();
  const sidebars = new Map();
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
      createStatusBarItem: () => ({ show() {}, hide() {}, dispose() {} }),
      onDidChangeActiveTextEditor: () => ({ dispose() {} }),
      activeTextEditor: { document: { uri: { scheme: "file", fsPath: source }, languageId: "python", isDirty: false } },
      createOutputChannel: () => ({ clear() { output = ""; }, append(s) { output += s; }, appendLine(s) { output += `${s}\n`; }, show() {}, dispose() {} }),
      showSaveDialog: async () => ({ fsPath: hex }),
      showOpenDialog: async () => selection,
      showErrorMessage: (s) => { messages.push(s); result(); },
      showInformationMessage: (s) => { messages.push(s); result(); },
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
    require("../extension").activate({ subscriptions: [], workspaceState: { get: () => undefined, update: async () => {} } });
  } finally {
    Module._load = original;
  }
  const actionTree = sidebars.get("pypic.actions");
  const sections = actionTree.getChildren();
  assert.deepEqual(sections.map((section) => section.label), ["Proyecto", "Construir", "Entorno"]);
  assert.deepEqual(actionTree.getChildren(sections[0]).map((action) => action.command), ["pypic.welcome", "pypic.newProject", "pypic.openProject", "pypic.openFile", "pypic.examples"]);
  assert.deepEqual(actionTree.getChildren(sections[1]).map((action) => action.command), ["pypic.check", "pypic.build", "pypic.selectTarget", "pypic.projectCheck", "pypic.status"]);
  assert.deepEqual(actionTree.getChildren(sections[2]).map((action) => action.command), ["pypic.selectCompiler", "pypic.selectDevices", "pypic.selectPicAs", "pypic.settings"]);
  let done = completed();
  await commands.get("pypic.check")();
  await done;
  assert.match(messages.pop(), /comprobación terminada/);
  done = completed();
  await commands.get("pypic.build")();
  await done;
  assert.match(messages.pop(), /compilación terminada/);
  assert.match(fs.readFileSync(hex, "utf8"), /^:/);
  fs.writeFileSync(source, "def main(\n");
  done = completed();
  await commands.get("pypic.check")();
  await done;
  assert.match(messages.pop(), /no pudo completar/);
  assert.match(output, /error\[PYP/);
  assert.ok(problems.some((problem) => problem.severity === 0 && String(problem.code).startsWith("PYP")));
  vscode.window.activeTextEditor.document.languageId = "pypic";
  const checked = new Promise((resolve) => { problemsUpdated = resolve; });
  saved(vscode.window.activeTextEditor.document);
  await checked;
  assert.ok(problems.some((problem) => problem.severity === 0));
  selection = undefined;
  await commands.get("pypic.selectCompiler")();
  assert.deepEqual(updates, []);
  selection = [{ fsPath: binary }];
  await commands.get("pypic.selectCompiler")();
  assert.deepEqual(updates[0], ["executable", binary, 3]);
  selection = [{ fsPath: path.join(root, "devices") }];
  await commands.get("pypic.selectDevices")();
  assert.deepEqual(updates[1], ["devicesDir", path.join(root, "devices"), 3]);
});

test("identifica PyPIC y conserva ubicación y severidad de diagnósticos", () => {
  const language = require("../language");
  assert.equal(language.isPyPic("from pypic.periph.adc import ADC\n"), true);
  assert.equal(language.isPyPic("from math import sin\n"), false);
  assert.deepEqual(language.diagnostics("C:\\x.py:3:4: warning[PYP3001]: aviso", "C:\\x.py"), [
    { line: 2, column: 3, severity: "warning", code: "PYP3001", message: "aviso" },
  ]);
  assert.match(language.info.uint16, /16 bits/);
  for (const name of ["CLOCK", "Internal", "Crystal", "ExternalClock", "MHz", "kHz", "Hz"]) {
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
  const tools = language.projectTools(path.join(root, "examples", "uart", "adc_serial.py"), path.dirname(root), "pypic", "");
  assert.equal(tools.executable, path.join(root, "target", "debug", process.platform === "win32" ? "pypic.exe" : "pypic"));
  assert.equal(tools.devices, path.join(root, "devices"));
  const workspaceTools = language.projectTools(root, root, "pypic", "");
  assert.equal(workspaceTools.executable, path.join(root, "target", "debug", process.platform === "win32" ? "pypic.exe" : "pypic"));
  assert.equal(workspaceTools.devices, path.join(root, "devices"));
  assert.equal(language.projectTools(path.join(root, "examples", "blink.py"), root, "custom-compiler", "custom-devices").executable, "custom-compiler");
});

test("la plantilla de proyecto para cristal compila con la CLI", () => {
  const project = require("../project");
  const source = project.source("PIC16F887", "Crystal[20 * MHz]");
  const root = path.resolve(__dirname, "../..");
  const binary = path.join(root, "target", "debug", process.platform === "win32" ? "pypic.exe" : "pypic");
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "pypic-project-template-"));
  const main = path.join(dir, "main.py");
  fs.writeFileSync(main, source);
  const result = execFileSync(binary, ["check", main, "--devices-dir", path.join(root, "devices")], { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
  assert.match(result, /main\.py: ok/);
});
