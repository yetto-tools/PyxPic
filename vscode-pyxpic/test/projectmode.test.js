"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const Module = require("node:module");
const { execFileSync } = require("node:child_process");

const root = path.resolve(__dirname, "../..");
const binary = path.join(root, "target", "debug", process.platform === "win32" ? "pyxpic.exe" : "pyxpic");
const devices = path.join(root, "devices");
const modes = require("../projectmode");

function temp(name) {
  return fs.mkdtempSync(path.join(os.tmpdir(), `pyxpic-${name}-`));
}

/** Un proyecto real creado por la CLI (`pyxpic init`), con la carpeta de Device Packs del repositorio. */
function makeProject(name, extra = []) {
  const dir = path.join(temp("proyecto"), name);
  execFileSync(binary, ["init", dir, "--name", name, "--devices-dir", devices.replace(/\\/g, "/"), ...extra], { cwd: root, stdio: "pipe" });
  return dir;
}

// ---- la lógica pura -----------------------------------------------------------------------------------------------------

test("el modo sale de si hay un pyxpic.toml cerca y de cómo se abrió el archivo", () => {
  const dir = temp("modo");
  const nested = path.join(dir, "a", "b");
  fs.mkdirSync(nested, { recursive: true });
  const file = path.join(nested, "main.py");
  fs.writeFileSync(file, "");
  assert.equal(modes.findProject(file), undefined, "sin pyxpic.toml es un archivo suelto");
  fs.writeFileSync(path.join(dir, "pyxpic.toml"), "");
  assert.equal(modes.findProject(file), path.join(dir, "pyxpic.toml"), "se sube por las carpetas hasta encontrarlo");
  assert.equal(modes.findProject(nested), path.join(dir, "pyxpic.toml"), "vale también una carpeta");
  assert.deepEqual(modes.resolve(file), { mode: "project", file, projectFile: path.join(dir, "pyxpic.toml"), target: undefined });
  assert.deepEqual(modes.resolve(file, { target: "cristal" }).target, "cristal");
  assert.deepEqual(modes.resolve(file, { standalone: true }), { mode: "file", file }, "abrir un archivo suelto gana al pyxpic.toml cercano");
  const solo = path.join(temp("solo"), "x.py");
  assert.deepEqual(modes.resolve(solo), { mode: "file", file: solo });
});

test("los argumentos de la CLI: archivo suelto con salida; proyecto con el perfil y sin salida", () => {
  const file = path.join(os.tmpdir(), "x.py");
  const solo = { mode: "file", file };
  assert.deepEqual(modes.invocation("check", solo), ["check", file]);
  assert.deepEqual(modes.invocation("build", solo, { output: "o.hex", devices: "d" }), ["build", file, "-o", "o.hex", "--devices-dir", "d"]);
  assert.throws(() => modes.invocation("build", solo), /ruta de salida/);
  const toml = path.join(os.tmpdir(), "pyxpic.toml");
  const project = { mode: "project", file, projectFile: toml };
  assert.deepEqual(modes.invocation("build", project), ["build", "--project", toml], "la salida es la del perfil: no se pasa -o");
  assert.deepEqual(modes.invocation("build", { ...project, target: "cristal" }, { devices: "d" }), ["build", "--project", toml, "--target", "cristal", "--devices-dir", "d"]);
  assert.deepEqual(modes.invocation("check", { ...project, target: "cristal" }), ["check", file, "--project", toml, "--target", "cristal"], "se comprueba ESTE archivo con el perfil");
});

test("la barra de estado y el resumen de la CLI", () => {
  assert.match(modes.statusText({ mode: "file" }), /archivo suelto/);
  assert.match(modes.statusText({ mode: "project", projectFile: "x" }, "demo"), /proyecto demo$/);
  assert.match(modes.statusText({ mode: "project", projectFile: "x", target: "cristal" }, "demo"), /proyecto demo · cristal$/);
  assert.match(modes.statusTooltip({ mode: "file" }), /pregunta dónde guardar/);
  assert.match(modes.statusTooltip({ mode: "project", projectFile: "p" }), /no pregunta dónde guardar/);
  assert.equal(modes.outputFile("Compilacion completada:\n  Archivo:       C:\\a b\\build\\x.hex\n  Programa:      30 palabras\n"), "C:\\a b\\build\\x.hex");
  assert.equal(modes.outputFile("nada"), undefined);
});

test("el informe JSON de la CLI real se lee y cada perfil se puede elegir", () => {
  const dir = makeProject("informe", ["--clock", "Crystal[20 * MHz]"]);
  const out = execFileSync(binary, ["project", "check", "--project", path.join(dir, "pyxpic.toml"), "--format", "json"], { cwd: root, encoding: "utf8" });
  const report = modes.parseReport(out);
  assert.equal(report.ok, true);
  assert.equal(report.name, "informe");
  assert.equal(report.targets.length, 1);
  assert.equal(report.targets[0].name, "pic16f887");
  assert.match(report.targets[0].clock, /Crystal 20 MHz \(FOSC = HS\)/);
  const item = modes.targetLabel(report.targets[0]);
  assert.match(item.label, /check.*pic16f887/);
  assert.match(item.description, /PIC16F887 · Crystal 20 MHz/);
  assert.equal(item.target, "pic16f887");
  assert.throws(() => modes.parseReport(JSON.stringify({ version: 2, targets: [] })), /desconocido/);
  assert.throws(() => modes.parseReport("no es json"));
});

// ---- los comandos de la extensión, con la CLI real -------------------------------------------------------------------------

function harness(initialFile) {
  const state = {
    commands: new Map(), executed: [], messages: [], errors: [], saveDialogs: 0, saveTo: undefined, openQueue: [], pickQueue: [], pickItems: [],
    outputText: "", shows: [], status: { text: "", visible: false }, shown: [], inputs: [], errorAction: undefined, workspaceFolders: undefined,
    workspaceState: new Map(), problems: [], done: undefined,
  };
  const activeDocument = { uri: { scheme: "file", fsPath: initialFile }, languageId: "python", isDirty: false, version: 1 };
  state.activeDocument = activeDocument;
  const settle = () => state.done && state.done();
  const Uri = { file: (fsPath) => ({ fsPath, toString: () => `file:${fsPath}` }), joinPath: (base, ...parts) => Uri.file(path.join(base.fsPath, ...parts)) };
  const vscode = {
    Uri,
    TreeItem: class { constructor(label) { this.label = label; } },
    TreeItemCollapsibleState: { None: 0, Collapsed: 1 },
    ThemeIcon: class { constructor(id) { this.id = id; } },
    Range: class { constructor(...args) { this.args = args; } },
    Diagnostic: class { constructor(range, message, severity) { Object.assign(this, { range, message, severity }); } },
    DiagnosticSeverity: { Error: 0, Warning: 1 },
    Hover: class {},
    CompletionItem: class { constructor(label) { this.label = label; } },
    ConfigurationTarget: { WorkspaceFolder: 3, Global: 1 },
    StatusBarAlignment: { Left: 1 },
    languages: {
      createDiagnosticCollection: () => ({ set(_u, items) { state.problems = items; }, delete() {}, dispose() {} }),
      registerHoverProvider: () => ({ dispose() {} }),
      registerCompletionItemProvider: () => ({ dispose() {} }),
    },
    window: {
      activeTextEditor: { document: activeDocument },
      registerTreeDataProvider: () => ({ dispose() {} }),
      registerWebviewViewProvider: () => ({ dispose() {} }),
      onDidChangeActiveTextEditor: () => ({ dispose() {} }),
      createStatusBarItem: () => ({
        set text(value) { state.status.text = value; }, get text() { return state.status.text; }, set tooltip(value) { state.status.tooltip = value; },
        show() { state.status.visible = true; }, hide() { state.status.visible = false; }, dispose() {},
      }),
      createOutputChannel: () => ({
        clear() { state.outputText = ""; }, append(s) { state.outputText += s; }, appendLine(s) { state.outputText += `${s}\n`; },
        show(...args) { state.shows.push({ args, text: state.outputText }); }, dispose() {},
      }),
      showSaveDialog: async () => { state.saveDialogs += 1; return state.saveTo && { fsPath: state.saveTo }; },
      showOpenDialog: async () => state.openQueue.shift(),
      showQuickPick: async (items) => { state.pickItems = items; const wanted = state.pickQueue.shift(); return typeof wanted === "function" ? items.find(wanted) : wanted; },
      showInputBox: async () => state.inputs.shift(),
      showTextDocument: async (uri) => { state.shown.push(uri.fsPath); },
      showErrorMessage: async (message) => { state.errors.push(message); settle(); return state.errorAction; },
      showInformationMessage: async (message) => { state.messages.push(message); settle(); },
    },
    workspace: {
      isTrusted: true,
      textDocuments: [],
      get workspaceFolders() { return state.workspaceFolders; },
      onDidOpenTextDocument: () => ({ dispose() {} }),
      onDidChangeTextDocument: () => ({ dispose() {} }),
      onDidCloseTextDocument: () => ({ dispose() {} }),
      onDidSaveTextDocument: () => ({ dispose() {} }),
      getConfiguration: () => ({ get: (key) => (key === "executable" ? binary : key === "devicesDir" ? devices : key === "checkOnSave" ? true : ""), update: async () => {} }),
      getWorkspaceFolder: () => undefined,
      fs: {
        stat: async (uri) => fs.statSync(uri.fsPath),
        createDirectory: async (uri) => fs.mkdirSync(uri.fsPath, { recursive: true }),
        writeFile: async (uri, bytes) => fs.writeFileSync(uri.fsPath, bytes),
      },
    },
    commands: {
      registerCommand: (name, fn) => { state.commands.set(name, fn); return { dispose() {} }; },
      executeCommand: async (name, ...args) => { state.executed.push([name, ...args]); },
    },
  };
  const original = Module._load;
  Module._load = function load(request, parent, main) { return request === "vscode" ? vscode : original.call(this, request, parent, main); };
  try {
    delete require.cache[require.resolve("../extension")];
    require("../extension").activate({ subscriptions: [], extensionUri: Uri.file(path.join(__dirname, "..")), workspaceState: { get: (k) => state.workspaceState.get(k), update: async (k, v) => { state.workspaceState.set(k, v); } } });
  } finally {
    Module._load = original;
  }
  state.run = async (command) => {
    const done = new Promise((resolve) => { state.done = resolve; });
    await state.commands.get(command)();
    await done;
    state.done = undefined;
  };
  return state;
}

test("en un proyecto, compilar usa el perfil, no pregunta dónde guardar y abre el panel de salida", async () => {
  const dir = makeProject("termometro");
  const h = harness(path.join(dir, "main.py"));
  assert.match(h.status.text, /proyecto termometro/, "la barra de estado dice que es un proyecto");
  assert.equal(h.status.visible, true);
  await h.run("pyxpic.build");
  assert.equal(h.saveDialogs, 0, "un proyecto no pregunta dónde guardar");
  const hex = path.join(dir, "build", "termometro.hex");
  assert.ok(fs.existsSync(hex), "la salida es la del perfil: build/termometro.hex");
  assert.match(fs.readFileSync(hex, "utf8"), /^:/);
  assert.match(h.messages.pop(), /compilación terminada: .*termometro\.hex/, "el aviso dice dónde quedó el .hex");
  // el panel Salida se abre y se selecciona al empezar y de nuevo al terminar, sin quitar el foco del editor
  assert.ok(h.shows.length >= 2, "show al empezar y al terminar");
  assert.ok(h.shows.every((s) => s.args[0] === true), "preserveFocus: el editor conserva el foco");
  assert.match(h.shows.at(-1).text, /Proyecto: .*pyxpic\.toml/);
  assert.match(h.shows.at(-1).text, /Perfil:\s+pic16f887/);
  assert.match(h.shows.at(-1).text, /Archivo:\s+.*termometro\.hex/, "el resultado ya está en el panel cuando se muestra al terminar");
});

test("comprobar también abre el panel de salida y usa el dispositivo y el reloj del perfil", async () => {
  const dir = makeProject("revisado", ["--clock", "Crystal[20 * MHz]"]);
  const h = harness(path.join(dir, "main.py"));
  await h.run("pyxpic.check");
  assert.match(h.messages.pop(), /comprobación terminada/);
  assert.ok(h.shows.length >= 2 && h.shows.every((s) => s.args[0] === true));
  assert.match(h.shows.at(-1).text, /main\.py: ok/);
});

test("un archivo abierto con Abrir archivo es un archivo suelto: compilar pregunta dónde guardar", async () => {
  const dir = makeProject("suelto");
  const main = path.join(dir, "main.py");
  const h = harness(main);
  assert.match(h.status.text, /proyecto suelto/);
  h.openQueue.push([{ fsPath: main }]);
  await h.commands.get("pyxpic.openFile")();
  assert.deepEqual(h.shown, [main]);
  assert.match(h.status.text, /archivo suelto/, "la barra de estado cambia a archivo suelto");
  assert.match(h.messages.pop(), /Archivo suelto: al compilar se pregunta dónde guardar/);
  const chosen = path.join(temp("salida"), "elegido.hex");
  h.saveTo = chosen;
  await h.run("pyxpic.build");
  assert.equal(h.saveDialogs, 1, "como siempre: pregunta dónde guardar");
  assert.ok(fs.existsSync(chosen));
  assert.ok(!fs.existsSync(path.join(dir, "build")), "no se usó la salida del perfil");
  assert.match(h.shows.at(-1).text, /Archivo suelto/);
  assert.ok(h.shows.length >= 2);
});

test("el perfil de destino se elige con un selector y se usa al compilar", async () => {
  const dir = makeProject("placas");
  execFileSync(binary, ["project", "set", "--project", path.join(dir, "pyxpic.toml"), "--target", "cristal", "--create", "device=PIC16F887", "clock=Crystal[20 * MHz]", "output=build/cristal.hex"], { cwd: root, stdio: "pipe" });
  execFileSync(binary, ["project", "set", "--project", path.join(dir, "pyxpic.toml"), "project.default_target=pic16f887"], { cwd: root, stdio: "pipe" });
  const h = harness(path.join(dir, "main.py"));
  h.pickQueue.push((item) => item.target === "cristal");
  await h.commands.get("pyxpic.selectTarget")();
  assert.deepEqual(h.pickItems.map((i) => i.target).sort(), ["cristal", "pic16f887"]);
  assert.match(h.pickItems.find((i) => i.target === "cristal").description, /Crystal 20 MHz/);
  assert.match(h.status.text, /proyecto placas · cristal/);
  await h.run("pyxpic.build");
  assert.ok(fs.existsSync(path.join(dir, "build", "cristal.hex")), "compiló el perfil elegido");
  assert.match(h.shows.at(-1).text, /perfil cristal/);
  assert.match(h.shows.at(-1).text, /Crystal 20 MHz/);
});

test("Abrir proyecto abre la carpeta; sin pyxpic.toml avisa y ofrece crearlo", async () => {
  const dir = makeProject("abierto");
  const h = harness(path.join(temp("otro"), "x.py"));
  h.openQueue.push([{ fsPath: dir }]);
  await h.commands.get("pyxpic.openProject")();
  assert.equal(h.executed.at(-1)[0], "vscode.openFolder");
  assert.equal(h.executed.at(-1)[1].fsPath, dir);
  assert.match(h.shows.at(-1).text, /Proyecto: .*pyxpic\.toml \(abierto\)/);
  assert.match(h.shows.at(-1).text, /perfil pic16f887: ok/);
  // una carpeta que no es un proyecto
  const empty = temp("vacia");
  h.openQueue.push([{ fsPath: empty }]);
  h.errorAction = undefined;
  await h.commands.get("pyxpic.openProject")();
  assert.match(h.errors.pop(), /No hay pyxpic\.toml en/);
  assert.ok(!fs.existsSync(path.join(empty, "pyxpic.toml")), "no se crea nada si no lo pides");
  h.openQueue.push([{ fsPath: empty }]);
  h.errorAction = "Crear proyecto aquí";
  await h.commands.get("pyxpic.openProject")();
  assert.ok(fs.existsSync(path.join(empty, "pyxpic.toml")) && fs.existsSync(path.join(empty, "main.py")), "ahora sí: la CLI crea el proyecto");
  assert.equal(h.executed.at(-1)[1].fsPath, empty);
});

test("Crear proyecto crea un proyecto de verdad con pyxpic.toml", async () => {
  const parent = temp("crear");
  const h = harness(path.join(parent, "x.py"));
  h.inputs.push("bateria");
  h.openQueue.push([{ fsPath: parent }]);
  h.pickQueue.push((item) => item.label === "PIC16F887", (item) => item.value === "Crystal[20 * MHz]");
  await h.commands.get("pyxpic.newProject")();
  const created = path.join(parent, "bateria");
  const toml = fs.readFileSync(path.join(created, "pyxpic.toml"), "utf8");
  assert.match(toml, /name = "bateria"/);
  assert.match(toml, /clock = "Crystal\[20 \* MHz\]"/);
  assert.ok(fs.existsSync(path.join(created, "main.py")));
  const check = execFileSync(binary, ["project", "check", "--project", path.join(created, "pyxpic.toml")], { cwd: root, encoding: "utf8" });
  assert.match(check, /perfil pic16f887: ok/);
});

test("el menú de la barra de estado cambia entre proyecto y archivo suelto", async () => {
  const dir = makeProject("menu");
  const h = harness(path.join(dir, "main.py"));
  h.pickQueue.push((item) => /archivo suelto/.test(item.label));
  await h.commands.get("pyxpic.mode")();
  assert.deepEqual(h.pickItems.map((i) => i.label.replace(/^\$\([a-z-]+\) /, "")), ["Cambiar perfil de destino", "Tratar este archivo como archivo suelto", "Abrir archivo…", "Abrir proyecto…", "Crear proyecto…"]);
  assert.match(h.status.text, /archivo suelto/);
  h.pickQueue.push((item) => /Usar el proyecto/.test(item.label));
  await h.commands.get("pyxpic.mode")();
  assert.match(h.status.text, /proyecto menu/);
});
