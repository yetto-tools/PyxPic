"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const Module = require("node:module");

test("los comandos comprueban y compilan con la CLI real y muestran sus errores", async () => {
  const root = path.resolve(__dirname, "../..");
  const binary = path.join(root, "target", "debug", process.platform === "win32" ? "pypic.exe" : "pypic");
  assert.ok(fs.existsSync(binary), "ejecuta cargo build -p pypic-cli antes de las pruebas");
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "pypic extension "));
  const source = path.join(dir, "blink.py");
  const hex = path.join(dir, "blink.hex");
  fs.copyFileSync(path.join(root, "examples", "blink.py"), source);
  const commands = new Map();
  let sidebar;
  const messages = [];
  let result;
  let output = "";
  const completed = () => new Promise((resolve) => { result = resolve; });
  const vscode = {
    Uri: { file: (fsPath) => ({ fsPath }) },
    TreeItem: class { constructor(label) { this.label = label; } },
    TreeItemCollapsibleState: { None: 0 },
    ThemeIcon: class { constructor(id) { this.id = id; } },
    window: {
      registerTreeDataProvider: (_id, provider) => { sidebar = provider; return { dispose() {} }; },
      activeTextEditor: { document: { uri: { scheme: "file", fsPath: source }, languageId: "python", isDirty: false } },
      createOutputChannel: () => ({ clear() { output = ""; }, append(s) { output += s; }, appendLine(s) { output += `${s}\n`; }, show() {}, dispose() {} }),
      showSaveDialog: async () => ({ fsPath: hex }),
      showErrorMessage: (s) => { messages.push(s); result(); },
      showInformationMessage: (s) => { messages.push(s); result(); },
    },
    workspace: {
      isTrusted: true,
      getConfiguration: () => ({ get: (key) => key === "executable" ? binary : path.join(root, "devices") }),
      getWorkspaceFolder: () => ({ uri: { fsPath: root } }),
    },
    commands: { registerCommand: (name, fn) => { commands.set(name, fn); return { dispose() {} }; } },
  };
  const original = Module._load;
  Module._load = function(request, parent, main) { return request === "vscode" ? vscode : original.call(this, request, parent, main); };
  try {
    require("../extension").activate({ subscriptions: [] });
  } finally {
    Module._load = original;
  }
  assert.deepEqual(sidebar.getChildren().map((item) => item.command.command), ["pypic.check", "pypic.build", "pypic.examples", "pypic.settings"]);
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
});
