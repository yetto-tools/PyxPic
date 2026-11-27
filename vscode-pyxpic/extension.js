"use strict";

const vscode = require("vscode");
const fs = require("node:fs");
const path = require("node:path");
const { execFile, spawn } = require("node:child_process");
const language = require("./language");
const project = require("./project");
const modes = require("./projectmode");
const sidebar = require("./sidebar");
const welcome = require("./welcome");
const pinout = require("./pinout");
const resources = require("./resources");
const variables = require("./variables");
const serial = require("./serial");
const extensionVersion = require("./package.json").version;
const { registerSpelling } = require("./spelling");

const welcomeCommands = Object.freeze({
  newProject: "pyxpic.newProject",
  openFile: "pyxpic.openFile",
  openProject: "pyxpic.openProject",
  openExample: "pyxpic.examples",
  check: "pyxpic.check",
  selectCompiler: "pyxpic.selectCompiler",
  selectDevices: "pyxpic.selectDevices",
  selectPicAs: "pyxpic.selectPicAs",
});

/** Lenguajes de programa de PyxPic: Python (`.py`, detectado como pyxpic al importar el SDK) y PyxPic BASIC (`.bas`). */
const SOURCE_LANGUAGES = ["python", "pyxpic", "pyxpic-basic"];

function activate(context) {
  const output = vscode.window.createOutputChannel("PyxPic");
  const diagnostics = vscode.languages.createDiagnosticCollection("PyxPic");
  const checks = new Map();
  let welcomePanel;
  context.subscriptions.push(output, diagnostics);
  registerSpelling(vscode, context.extensionPath, (message) => output.appendLine(message));

  // Archivo suelto o proyecto (projectmode.js): un archivo abierto con "Abrir archivo" se trata como suelto aunque haya un pyxpic.toml cerca.
  const standalone = new Set();
  const statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
  statusBar.command = "pyxpic.mode";
  // botones de la barra de estado: verificar, compilar y limpiar, visibles mientras hay un archivo PyxPic activo
  const buttons = [
    ["$(check) Verificar", "pyxpic.check", "PyxPic: verificar el código (pyxpic check)"],
    ["$(tools) Compilar", "pyxpic.build", "PyxPic: compilar a HEX (pyxpic build)"],
    ["$(trash) Limpiar", "pyxpic.clean", "PyxPic: borrar los .hex que PyxPic generó"],
  ].map(([text, command, tooltip], index) => {
    const item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 99 - index);
    Object.assign(item, { text, command, tooltip });
    return item;
  });
  context.subscriptions.push(statusBar, ...buttons);
  const targetKey = (projectFile) => `pyxpic.target:${projectFile}`;

  function contextFor(document) {
    const file = document.uri.fsPath;
    const projectFile = modes.findProject(file);
    return modes.resolve(file, { standalone: standalone.has(file), target: projectFile && context.workspaceState.get(targetKey(projectFile)) });
  }

  function refreshStatus() {
    const editor = vscode.window.activeTextEditor;
    const document = editor && editor.document;
    if (!document || document.uri.scheme !== "file" || !SOURCE_LANGUAGES.includes(document.languageId)) {
      statusBar.hide();
      buttons.forEach((button) => button.hide());
      return;
    }
    const ctx = contextFor(document);
    statusBar.text = modes.statusText(ctx, ctx.projectFile && path.basename(path.dirname(ctx.projectFile)));
    statusBar.tooltip = modes.statusTooltip(ctx);
    statusBar.show();
    buttons.forEach((button) => button.show());
  }
  context.subscriptions.push(vscode.window.onDidChangeActiveTextEditor(refreshStatus));

  /** Un diagnóstico de la CLI como el de VS Code: el error, su causa y su solución en el mensaje, y qué analizador lo encontró en el origen. */
  function toDiagnostic(item) {
    const hasEnd = Number.isInteger(item.endLine) && (item.endLine > item.line || item.endColumn > item.column);
    const range = new vscode.Range(item.line, item.column, hasEnd ? item.endLine : item.line, hasEnd ? item.endColumn : item.column + 1);
    const severity = item.severity === "error" ? vscode.DiagnosticSeverity.Error : vscode.DiagnosticSeverity.Warning;
    const diagnostic = new vscode.Diagnostic(range, language.describeDiagnostic(item), severity);
    diagnostic.code = item.code;
    diagnostic.source = language.diagnosticSource(item);
    return diagnostic;
  }

  /** Ejecuta la CLI con `text` por la entrada estándar (el texto del editor, guardado o no); no lanza. */
  function cliInput(tools, args, cwd, text) {
    return new Promise((resolve) => {
      const child = spawn(tools.executable, args, { cwd });
      let stdout = "";
      let stderr = "";
      child.stdout.on("data", (chunk) => { stdout += chunk; });
      child.stderr.on("data", (chunk) => { stderr += chunk; });
      child.on("error", (error) => resolve({ error, stdout, stderr }));
      child.on("close", (code) => resolve({ error: code ? new Error(`salida ${code}`) : undefined, stdout, stderr }));
      child.stdin.on("error", () => {});
      child.stdin.end(text);
    });
  }

  // El análisis mientras se escribe: el analizador sintáctico y el semántico de la CLI revisan el texto del editor tras una pausa corta, sin guardar.
  const liveTimers = new Map();
  async function liveCheck(document) {
    const config = vscode.workspace.getConfiguration("pyxpic", document.uri);
    if (!config.get("liveCheck", true) || document.uri.scheme !== "file" || !["pyxpic", "pyxpic-basic"].includes(document.languageId)) return;
    const version = document.version;
    const folder = vscode.workspace.getWorkspaceFolder(document.uri);
    const cwd = folder ? folder.uri.fsPath : path.dirname(document.uri.fsPath);
    const tools = language.projectTools(document.uri.fsPath, cwd, config.get("executable", "pyxpic"), config.get("devicesDir", "").trim());
    const args = ["check", "-", "--lang", document.languageId === "pyxpic-basic" ? "bas" : "py", "--format", "json", ...(tools.devices ? ["--devices-dir", tools.devices] : [])];
    const result = await cliInput(tools, args, cwd, document.getText());
    // un resultado de una versión vieja del texto no se muestra
    if (document.isClosed || document.version !== version) return;
    const items = language.diagnosticsFromJson(result.stdout);
    if (items) diagnostics.set(document.uri, items.map(toDiagnostic));
  }
  function scheduleLive(document) {
    const key = document.uri.toString();
    clearTimeout(liveTimers.get(key));
    liveTimers.set(key, setTimeout(() => { liveTimers.delete(key); void liveCheck(document); }, 500));
  }
  context.subscriptions.push({ dispose: () => liveTimers.forEach((timer) => clearTimeout(timer)) });

  /** Ejecuta la CLI y devuelve su salida; no lanza: el código de salida distinto de 0 es parte del resultado. */
  function cli(tools, args, cwd) {
    return new Promise((resolve) => {
      execFile(tools.executable, args, { cwd, maxBuffer: 4 * 1024 * 1024 }, (error, stdout, stderr) => resolve({ error, stdout: String(stdout), stderr: String(stderr) }));
    });
  }

  function detect(document) {
    if (document.languageId === "python" && language.isPyxPic(document.getText())) {
      void vscode.languages.setTextDocumentLanguage(document, "pyxpic");
    }
  }
  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument(detect),
    vscode.workspace.onDidChangeTextDocument((event) => {
      // sin el analisis en vivo, lo que se muestra deja de corresponder al texto en cuanto cambia
      if (!vscode.workspace.getConfiguration("pyxpic", event.document.uri).get("liveCheck", true)) diagnostics.delete(event.document.uri);
      detect(event.document);
      if (event.contentChanges.length) scheduleLive(event.document);
      if (event.contentChanges.length) resourcesView.markStale(event.document.uri.toString());
    }),
    vscode.workspace.onDidCloseTextDocument((document) => { diagnostics.delete(document.uri); checks.delete(document.uri.toString()); }),
    vscode.workspace.onDidSaveTextDocument((document) => {
      if (["pyxpic", "pyxpic-basic"].includes(document.languageId) && vscode.workspace.getConfiguration("pyxpic", document.uri).get("checkOnSave", true)) void run("check", document, true);
      pinoutView.refresh();
    }),
    vscode.languages.registerHoverProvider("pyxpic", {
      provideHover(document, position) {
        const range = document.getWordRangeAtPosition(position);
        const word = range && document.getText(range);
        return word && Object.hasOwn(language.info, word) ? new vscode.Hover(language.info[word]) : undefined;
      },
    }),
    vscode.languages.registerHoverProvider("pyxpic-basic", {
      provideHover(document, position) {
        const range = document.getWordRangeAtPosition(position);
        const word = range && document.getText(range);
        const key = word && word.toLowerCase();
        if (key && Object.hasOwn(language.basicInfo, key)) return new vscode.Hover(language.basicInfo[key]);
        return word && Object.hasOwn(language.info, word.toUpperCase()) ? new vscode.Hover(language.info[word.toUpperCase()]) : undefined;
      },
    }),
    vscode.languages.registerCompletionItemProvider("pyxpic", {
      provideCompletionItems(document, position) {
        const prefix = document.lineAt(position.line).text.slice(0, position.character);
        return language.completions(prefix).map((name) => {
          const item = new vscode.CompletionItem(name);
          item.detail = language.info[name] || "PyxPic";
          return item;
        });
      },
    }, "."),
    vscode.languages.registerCompletionItemProvider("pyxpic-basic", {
      provideCompletionItems(document, position) {
        const prefix = document.lineAt(position.line).text.slice(0, position.character);
        return language.basicCompletions(prefix).map((name) => {
          const item = new vscode.CompletionItem(name);
          item.detail = language.basicInfo[name.toLowerCase()] || language.info[name] || "PyxPic BASIC";
          return item;
        });
      },
    }, "."),
  );
  for (const document of vscode.workspace.textDocuments) detect(document);

  sidebar.register(vscode, context);
  async function currentHardware() {
    const document = vscode.window.activeTextEditor?.document;
    const data = environment(document);
    const declared = document && language.declaredDevice(document.getText());
    const sourceDevice = pinout.detectedDevice(declared);
    if (sourceDevice) return { devicesDir: data.tools.devices, detected: sourceDevice };
    if (document?.uri.scheme === "file") {
      const projectContext = contextFor(document);
      if (projectContext.projectFile) {
        try {
          const report = await projectReport(projectContext.projectFile, projectContext.target);
          const projectDevice = pinout.detectedDevice(undefined, report, projectContext.target);
          if (projectDevice) return { devicesDir: data.tools.devices, detected: projectDevice };
          return { devicesDir: data.tools.devices, notice: "El proyecto no tiene un perfil de destino inequívoco." };
        } catch (error) {
          if (error instanceof Error) return { devicesDir: data.tools.devices, notice: `No se pudo leer el perfil del proyecto: ${error.message}` };
          throw error;
        }
      }
    }
    return { devicesDir: data.tools.devices };
  }
  const pinoutView = pinout.register(vscode, context, currentHardware);
  // la lista de variables de un .bas (como la del IDE de PICAXE): la CLI (`pyxpic vars`) lee el texto del editor, guardado o no
  const basicDocument = () => {
    const document = vscode.window.activeTextEditor?.document;
    return document && document.languageId === "pyxpic-basic" ? document : undefined;
  };
  const variablesView = variables.register(
    vscode,
    context,
    () => {
      const document = basicDocument();
      return document ? { file: path.basename(document.uri.fsPath || document.fileName), text: document.getText() } : undefined;
    },
    (text) => {
      const document = basicDocument();
      const config = vscode.workspace.getConfiguration("pyxpic", document?.uri);
      const folder = document && vscode.workspace.getWorkspaceFolder(document.uri);
      const cwd = folder ? folder.uri.fsPath : document ? path.dirname(document.uri.fsPath) : process.cwd();
      const tools = language.projectTools(document?.uri.fsPath || cwd, cwd, config.get("executable", "pyxpic"), config.get("devicesDir", "").trim());
      return new Promise((resolve) => {
        const child = spawn(tools.executable, ["vars", "-", "--format", "json"], { cwd });
        let stdout = "";
        let stderr = "";
        child.stdout.on("data", (chunk) => { stdout += chunk; });
        child.stderr.on("data", (chunk) => { stderr += chunk; });
        child.on("error", (error) => resolve({ error, stdout, stderr }));
        child.on("close", (code) => resolve({ error: code ? new Error(`salida ${code}`) : undefined, stdout, stderr }));
        child.stdin.on("error", () => {});
        child.stdin.end(text);
      });
    },
  );
  const showVariables = () => { void vscode.commands.executeCommand("setContext", "pyxpic.basicActive", Boolean(basicDocument())); variablesView.refresh(); };
  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor(showVariables),
    vscode.workspace.onDidChangeTextDocument((event) => { if (event.document === basicDocument() && event.contentChanges.length) variablesView.schedule(); }),
    vscode.commands.registerCommand("pyxpic.revealLine", async (line) => {
      const editor = vscode.window.activeTextEditor;
      if (!editor || !Number.isInteger(line)) return;
      const position = new vscode.Position(Math.max(0, line - 1), 0);
      editor.selection = new vscode.Selection(position, position);
      editor.revealRange(new vscode.Range(position, position), vscode.TextEditorRevealType.InCenter);
    }),
  );
  showVariables();
  const resourcesView = resources.register(
    vscode,
    context,
    async () => {
      const document = vscode.window.activeTextEditor?.document;
      const hardware = await currentHardware();
      return {
        devicesDir: hardware.devicesDir,
        deviceName: hardware.detected?.name,
        fileKey: document?.uri.toString(),
        source: document?.getText() || "",
      };
    },
    // la medicion: `pyxpic memory --format json` del archivo activo, con el mismo perfil y dispositivo que `build` (la CLI es la unica fuente de los numeros)
    async () => {
      const document = vscode.window.activeTextEditor?.document;
      if (!document || document.uri.scheme !== "file" || !SOURCE_LANGUAGES.includes(document.languageId)) return { error: new Error("sin archivo"), stdout: "", stderr: "" };
      const ctx = contextFor(document);
      const config = vscode.workspace.getConfiguration("pyxpic", document.uri);
      const folder = vscode.workspace.getWorkspaceFolder(document.uri);
      const cwd = ctx.mode === "project" ? path.dirname(ctx.projectFile) : folder ? folder.uri.fsPath : path.dirname(document.uri.fsPath);
      const tools = language.projectTools(document.uri.fsPath, folder ? folder.uri.fsPath : path.dirname(document.uri.fsPath), config.get("executable", "pyxpic"), config.get("devicesDir", "").trim());
      return cli(tools, [...modes.invocation("memory", ctx, { devices: tools.devices }), "--format", "json"], cwd);
    },
  );
  // la terminal serial: `pyxpic monitor` (la CLI abre el puerto; la extension solo lee sus eventos)
  const serialTools = () => {
    const document = vscode.window.activeTextEditor?.document;
    const config = vscode.workspace.getConfiguration("pyxpic", document?.uri);
    const folder = document && vscode.workspace.getWorkspaceFolder(document.uri);
    const cwd = folder ? folder.uri.fsPath : document ? path.dirname(document.uri.fsPath) : process.cwd();
    return { cwd, tools: language.projectTools(document?.uri.fsPath || cwd, cwd, config.get("executable", "pyxpic"), config.get("devicesDir", "").trim()) };
  };
  serial.register(vscode, context, {
    listPorts: () => { const { cwd, tools } = serialTools(); return cli(tools, ["monitor", "--list", "--format", "json"], cwd); },
    spawn: (args) => {
      const { cwd, tools } = serialTools();
      return spawn(tools.executable, tools.devices && args.includes("--sim") ? [...args, "--devices-dir", tools.devices] : args, { cwd });
    },
    activeSource: () => {
      const document = vscode.window.activeTextEditor?.document;
      return document && document.uri.scheme === "file" && SOURCE_LANGUAGES.includes(document.languageId) ? document.uri.fsPath : undefined;
    },
  });
  refreshStatus();

  // Limpiar solo borra los .hex que PyxPic dijo haber generado (el resumen de `pyxpic build`), nunca rutas adivinadas.
  const BUILT = "pyxpic.builtFiles";
  const builtFiles = () => (context.workspaceState.get(BUILT) || []).filter((file) => typeof file === "string" && file.toLowerCase().endsWith(".hex"));
  async function rememberBuilt(file) {
    const resolved = path.resolve(file);
    await context.workspaceState.update(BUILT, [...new Set([...builtFiles(), resolved])]);
  }

  async function clean() {
    const known = builtFiles();
    const existing = known.filter((file) => fs.existsSync(file));
    if (existing.length === 0) {
      if (known.length) await context.workspaceState.update(BUILT, []);
      void vscode.window.showInformationMessage("PyxPic: no hay archivos .hex generados que limpiar.");
      return;
    }
    const names = existing.map((file) => path.basename(file)).join(", ");
    const choice = await vscode.window.showWarningMessage(`PyxPic: se borrarán ${existing.length} archivo(s) .hex generados por la compilación: ${names}`, { modal: true, detail: existing.join(String.fromCharCode(10)) }, "Limpiar");
    if (choice !== "Limpiar") return;
    const failed = [];
    for (const file of existing) {
      try {
        fs.unlinkSync(file);
      } catch (error) {
        failed.push(`${file}: ${error.message}`);
      }
    }
    await context.workspaceState.update(BUILT, known.filter((file) => failed.some((line) => line.startsWith(`${file}:`))));
    output.clear();
    output.appendLine(`PyxPic clean: ${existing.length - failed.length} archivo(s) borrados${failed.length ? `, ${failed.length} con error` : ""}`);
    existing.filter((file) => !failed.some((line) => line.startsWith(`${file}:`))).forEach((file) => output.appendLine(`  borrado ${file}`));
    failed.forEach((line) => output.appendLine(`  no se pudo borrar ${line}`));
    void vscode.window.showInformationMessage(failed.length ? "PyxPic: no se pudieron borrar todos los archivos. Consulta la salida PyxPic." : `PyxPic: limpieza terminada (${existing.length} archivo(s)).`);
  }

  async function run(kind, targetDocument, quiet = false) {
    if (!vscode.workspace.isTrusted) {
      if (!quiet) void vscode.window.showErrorMessage("PyxPic necesita un espacio de trabajo de confianza para ejecutar el compilador.");
      return;
    }
    const editor = vscode.window.activeTextEditor;
    const document = targetDocument || (editor && editor.document);
    if (!document || document.uri.scheme !== "file" || !SOURCE_LANGUAGES.includes(document.languageId)) {
      void vscode.window.showErrorMessage("Abre un archivo PyxPic local (Python o BASIC) antes de continuar.");
      return;
    }
    if (document.isDirty && !(await document.save())) return;
    const file = document.uri.fsPath;
    const version = document.version;
    const ctx = contextFor(document);
    // un archivo suelto pregunta dónde guardar el .hex; un proyecto usa la salida de su perfil y no pregunta nada
    let saveTo;
    if (kind === "build" && ctx.mode === "file") {
      const suggested = vscode.Uri.file(path.join(path.dirname(file), `${path.parse(file).name}.hex`));
      const target = await vscode.window.showSaveDialog({ defaultUri: suggested, filters: { "Intel HEX": ["hex"] }, saveLabel: "Compilar" });
      if (!target) return;
      saveTo = target.fsPath;
    }
    const config = vscode.workspace.getConfiguration("pyxpic", document.uri);
    const folder = vscode.workspace.getWorkspaceFolder(document.uri);
    const cwd = ctx.mode === "project" ? path.dirname(ctx.projectFile) : folder ? folder.uri.fsPath : path.dirname(file);
    const tools = language.projectTools(file, folder ? folder.uri.fsPath : path.dirname(file), config.get("executable", "pyxpic"), config.get("devicesDir", "").trim());
    const args = modes.invocation(kind, ctx, { output: saveTo, devices: tools.devices });
    const key = document.uri.toString();
    const request = Symbol();
    checks.set(key, request);
    output.clear();
    output.appendLine(`PyxPic ${kind}: ${file}`);
    output.appendLine(ctx.mode === "project" ? `Proyecto: ${ctx.projectFile}${ctx.target ? ` (perfil ${ctx.target})` : ""}` : "Archivo suelto");
    // al comprobar o compilar se abre y se selecciona el panel Salida (canal PyxPic) sin quitar el foco del editor
    if (!quiet) output.show(true);
    execFile(tools.executable, args, { cwd, maxBuffer: 4 * 1024 * 1024 }, (error, stdout, stderr) => {
      if (stdout) output.append(stdout);
      if (stderr) output.append(stderr);
      if (!error) void resourcesView.measure();
      if (!quiet) output.show(true);
      if (checks.get(key) === request && document.version === version && !document.isClosed) {
        diagnostics.set(document.uri, language.diagnostics(`${stdout}\n${stderr}`, file).map(toDiagnostic));
      }
      if (error) {
        output.appendLine(`\nNo se completó: ${error.message}`);
        if (!quiet) void vscode.window.showErrorMessage("PyxPic no pudo completar la operación. Consulta la salida PyxPic y la ruta del ejecutable.");
      } else if (!quiet) {
        const hex = kind === "build" ? modes.outputFile(stdout) : undefined;
        if (hex) void rememberBuilt(hex);
        void vscode.window.showInformationMessage(kind === "build" ? `PyxPic: compilación terminada${hex ? `: ${hex}` : ""}.` : "PyxPic: comprobación terminada.");
      }
    });
  }

  async function examples() {
    const files = await vscode.workspace.findFiles("**/examples/**/*.{py,bas,s}", "**/{target,node_modules}/**", 200);
    if (!files.length) {
      void vscode.window.showInformationMessage("No se encontraron ejemplos. Abre la carpeta del repositorio PyxPic.");
      return;
    }
    const items = files.map((uri) => ({ label: vscode.workspace.asRelativePath(uri), uri }));
    const selected = await vscode.window.showQuickPick(items, { placeHolder: "Selecciona un ejemplo PyxPic" });
    if (selected) await vscode.window.showTextDocument(selected.uri);
  }

  function environment(document) {
    const uri = document && document.uri.scheme === "file" ? document.uri : undefined;
    const folder = uri && vscode.workspace.getWorkspaceFolder(uri);
    const cwd = folder ? folder.uri.fsPath : uri ? path.dirname(uri.fsPath) : vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || process.cwd();
    const config = vscode.workspace.getConfiguration("pyxpic", uri);
    const tools = language.projectTools(uri?.fsPath || cwd, cwd, config.get("executable", "pyxpic"), config.get("devicesDir", "").trim());
    const exists = (value) => value && require("node:fs").existsSync(value);
    const packsReady = tools.devices && exists(path.join(tools.devices, "pic16f887", "device.json"));
    const picAs = config.get("picAsPath", "");
    return {
      tools,
      rows: [
        { name: "Compilador PyxPic", value: tools.executable, detail: exists(tools.executable) ? "Ejecutable encontrado y listo para comprobar o compilar." : "Selecciona el ejecutable compilado o instala PyxPic en PATH.", state: exists(tools.executable) ? "Listo" : "Falta configurar", kind: exists(tools.executable) ? "ready" : "warning", action: "Elegir compilador", command: "selectCompiler" },
        { name: "Device Packs", value: tools.devices || "Búsqueda predeterminada", detail: packsReady ? "PIC16F887 está disponible para el proyecto." : "Selecciona la carpeta devices que contiene los Device Packs.", state: packsReady ? "Listo" : "Falta configurar", kind: packsReady ? "ready" : "warning", action: "Elegir carpeta", command: "selectDevices" },
        { name: "PIC-AS de XC8", value: picAs || "Sin configurar", detail: picAs ? "La ruta está registrada. La ejecución de PIC-AS desde la extensión sigue pendiente." : "Opcional: PyxPic Native no necesita XC8 ni PIC-AS.", state: picAs ? "Registrado" : "Opcional", kind: picAs ? "ready" : "warning", action: "Elegir PIC-AS", command: "selectPicAs" },
        { name: "Espacio de trabajo", value: vscode.workspace.isTrusted ? "Confiable" : "Modo restringido", detail: vscode.workspace.isTrusted ? "La extensión puede compilar y crear proyectos." : "Confía en la carpeta para habilitar compilación y creación de proyectos.", state: vscode.workspace.isTrusted ? "Listo" : "Requiere confianza", kind: vscode.workspace.isTrusted ? "ready" : "warning", action: "", command: "" },
      ],
      projectText: folder ? `Carpeta actual: ${folder.name}.` : "Elige una carpeta de proyecto o crea una nueva para empezar.",
      device: document ? language.declaredDevice(document.getText()) || "No se detectó un dispositivo declarado en el archivo activo." : "Abre un archivo PyxPic para detectar el dispositivo.",
    };
  }

  function renderWelcome() {
    if (!welcomePanel) return;
    const document = vscode.window.activeTextEditor && vscode.window.activeTextEditor.document;
    const data = environment(document);
    welcomePanel.webview.html = welcome.html(welcomePanel.webview, context.extensionUri, { tools: data.rows, projectText: data.projectText, device: data.device });
  }

  function openWelcome() {
    if (welcomePanel) {
      welcomePanel.reveal(vscode.ViewColumn.Active);
      renderWelcome();
      return;
    }
    welcomePanel = vscode.window.createWebviewPanel("pyxpic.welcome", "PyxPic: Inicio", vscode.ViewColumn.Active, { enableScripts: true, localResourceRoots: [vscode.Uri.joinPath(context.extensionUri, "media")] });
    welcomePanel.onDidDispose(() => { welcomePanel = undefined; }, undefined, context.subscriptions);
    welcomePanel.webview.onDidReceiveMessage(async (message) => {
      const command = message && message.command;
      if (command === "refresh") renderWelcome();
      else if (Object.hasOwn(welcomeCommands, command)) await vscode.commands.executeCommand(welcomeCommands[command]);
      renderWelcome();
    }, undefined, context.subscriptions);
    renderWelcome();
  }

  async function newProject() {
    if (!vscode.workspace.isTrusted) {
      void vscode.window.showErrorMessage("PyxPic necesita un espacio de trabajo de confianza para crear un proyecto.");
      return;
    }
    const { tools } = toolsFor(vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || process.cwd());
    // un proyecto de verdad: `pyxpic init` escribe pyxpic.toml (dispositivo, reloj y salida) y un main.py de partida valido
    const init = async (dir, name, device, clock) => {
      const result = await cli(tools, ["init", dir, "--name", name, "--device", device, "--clock", clock, ...(tools.devices ? ["--devices-dir", tools.devices] : [])], dir);
      output.clear();
      output.append(`${result.stdout}${result.stderr}`);
      if (result.error) {
        output.show(true);
        throw new Error(result.stderr.trim().split("\n")[0] || "pyxpic init fallo");
      }
    };
    const created = await project.create(vscode, init);
    if (!created) return;
    const action = await vscode.window.showInformationMessage(`Proyecto ${project.projectName(created.root)} creado para ${created.device}.`, "Abrir proyecto");
    if (action === "Abrir proyecto") await vscode.commands.executeCommand("vscode.openFolder", created.root, true);
    else await vscode.window.showTextDocument(created.main);
  }

  /** Los datos y el ejecutable para hablar con la CLI desde un archivo o desde la carpeta abierta. */
  function toolsFor(fsPath) {
    const folder = vscode.workspace.workspaceFolders && vscode.workspace.workspaceFolders[0];
    const cwd = folder ? folder.uri.fsPath : path.dirname(fsPath);
    const config = vscode.workspace.getConfiguration("pyxpic");
    return { tools: language.projectTools(fsPath, cwd, config.get("executable", "pyxpic"), config.get("devicesDir", "").trim()), cwd };
  }

  /** El proyecto del archivo activo o, si no hay, el de la carpeta abierta. */
  function activeProject() {
    const editor = vscode.window.activeTextEditor;
    const document = editor && editor.document;
    if (document && document.uri.scheme === "file") {
      const ctx = contextFor(document);
      if (ctx.projectFile) return ctx.projectFile;
    }
    const folder = vscode.workspace.workspaceFolders && vscode.workspace.workspaceFolders[0];
    return folder && modes.findProject(folder.uri.fsPath);
  }

  /** `pyxpic project check --format json`: el informe de los perfiles (sale con 1 si alguno tiene errores y aun asi imprime el JSON). */
  async function projectReport(projectFile, target) {
    const { tools } = toolsFor(projectFile);
    const args = ["project", "check", "--project", projectFile, "--format", "json"];
    if (target) args.push("--target", target);
    if (tools.devices) args.push("--devices-dir", tools.devices);
    const result = await cli(tools, args, path.dirname(projectFile));
    try {
      return modes.parseReport(result.stdout);
    } catch {
      throw new Error((result.stderr || result.stdout || "la CLI no devolvio un informe").trim());
    }
  }

  async function openFile() {
    const picked = await vscode.window.showOpenDialog({ canSelectFiles: true, canSelectFolders: false, canSelectMany: false, filters: { "Programa PyxPic (Python)": ["py"] }, openLabel: "Abrir archivo", title: "Abrir archivo PyxPic" });
    if (!picked || !picked[0]) return;
    standalone.add(picked[0].fsPath);
    await vscode.window.showTextDocument(picked[0]);
    refreshStatus();
    void vscode.window.showInformationMessage("Archivo suelto: al compilar se pregunta dónde guardar el .hex. Para usar un proyecto, elige Abrir proyecto.");
  }

  async function openProject() {
    const picked = await vscode.window.showOpenDialog({ canSelectFiles: false, canSelectFolders: true, canSelectMany: false, openLabel: "Abrir proyecto", title: "Abrir proyecto PyxPic (la carpeta que contiene pyxpic.toml)" });
    if (!picked || !picked[0]) return;
    const folder = picked[0].fsPath;
    const projectFile = path.join(folder, modes.PROJECT_FILE);
    if (!require("node:fs").existsSync(projectFile)) {
      const action = await vscode.window.showErrorMessage(`No hay ${modes.PROJECT_FILE} en ${folder}: no es un proyecto PyxPic.`, "Crear proyecto aquí");
      if (action !== "Crear proyecto aquí") return;
      const { tools } = toolsFor(folder);
      const created = await cli(tools, ["init", folder, ...(tools.devices ? ["--devices-dir", tools.devices] : [])], folder);
      output.clear();
      output.append(`${created.stdout}${created.stderr}`);
      output.show(true);
      if (created.error) {
        void vscode.window.showErrorMessage("PyxPic no pudo crear el proyecto. Consulta la salida PyxPic.");
        return;
      }
    }
    let report;
    try {
      report = await projectReport(projectFile);
    } catch (error) {
      output.clear();
      output.appendLine(error.message);
      output.show(true);
      void vscode.window.showErrorMessage(`No se pudo leer el proyecto: ${error.message.split("\n")[0]}`);
      return;
    }
    for (const file of [...standalone]) if (!path.relative(folder, file).startsWith("..")) standalone.delete(file);
    output.clear();
    output.appendLine(`Proyecto: ${projectFile}${report.name ? ` (${report.name})` : ""}`);
    for (const t of report.targets) output.appendLine(`  perfil ${t.name}: ${t.ok ? "ok" : "CON ERRORES"}${t.device ? ` · ${t.device}` : ""}${t.clock ? ` · ${t.clock}` : ""}`);
    output.show(true);
    const inside = vscode.workspace.workspaceFolders && vscode.workspace.workspaceFolders.some((f) => f.uri.fsPath === folder);
    if (inside) {
      if (report.main) await vscode.window.showTextDocument(vscode.Uri.file(report.main));
      refreshStatus();
    } else {
      await vscode.commands.executeCommand("vscode.openFolder", vscode.Uri.file(folder), false);
    }
  }

  async function selectTarget() {
    const projectFile = activeProject();
    if (!projectFile) {
      void vscode.window.showInformationMessage("No hay un proyecto PyxPic abierto: elige Abrir proyecto o Crear proyecto.");
      return;
    }
    let report;
    try {
      report = await projectReport(projectFile);
    } catch (error) {
      void vscode.window.showErrorMessage(`No se pudo leer el proyecto: ${error.message.split("\n")[0]}`);
      return;
    }
    if (!report.targets.length) {
      void vscode.window.showInformationMessage("El proyecto no define perfiles de destino. Crea uno con: pyxpic project set --target NOMBRE --create device=PIC16F887");
      return;
    }
    const picked = await vscode.window.showQuickPick(report.targets.map(modes.targetLabel), { title: "Perfil de destino", placeHolder: "Elige el dispositivo y el reloj con los que se compila" });
    if (!picked) return;
    await context.workspaceState.update(targetKey(projectFile), picked.target);
    pinoutView.refresh();
    refreshStatus();
    void vscode.window.showInformationMessage(`Perfil de destino: ${picked.target}.`);
  }

  async function projectCheck() {
    const projectFile = activeProject();
    if (!projectFile) {
      void vscode.window.showInformationMessage("No hay un proyecto PyxPic abierto: elige Abrir proyecto o Crear proyecto.");
      return;
    }
    const { tools } = toolsFor(projectFile);
    const args = ["project", "check", "--project", projectFile];
    const target = context.workspaceState.get(targetKey(projectFile));
    if (target) args.push("--target", target);
    if (tools.devices) args.push("--devices-dir", tools.devices);
    const result = await cli(tools, args, path.dirname(projectFile));
    output.clear();
    output.append(`${result.stdout}${result.stderr}`);
    output.show(true);
    void (result.error ? vscode.window.showErrorMessage("PyxPic: el proyecto tiene errores. Consulta la salida PyxPic.") : vscode.window.showInformationMessage("PyxPic: proyecto revisado, sin errores."));
  }

  async function modeMenu() {
    const editor = vscode.window.activeTextEditor;
    const document = editor && editor.document;
    const items = [];
    if (document && document.uri.scheme === "file") {
      const ctx = contextFor(document);
      if (ctx.mode === "project") {
        items.push({ label: "$(target) Cambiar perfil de destino", command: "pyxpic.selectTarget" });
        items.push({ label: "$(file-code) Tratar este archivo como archivo suelto", action: () => { standalone.add(document.uri.fsPath); refreshStatus(); } });
      } else if (modes.findProject(document.uri.fsPath)) {
        items.push({ label: "$(circuit-board) Usar el proyecto de este archivo", action: () => { standalone.delete(document.uri.fsPath); refreshStatus(); } });
      }
    }
    items.push({ label: "$(file) Abrir archivo…", command: "pyxpic.openFile" }, { label: "$(folder-opened) Abrir proyecto…", command: "pyxpic.openProject" }, { label: "$(new-file) Crear proyecto…", command: "pyxpic.newProject" });
    const picked = await vscode.window.showQuickPick(items, { placeHolder: "PyxPic: archivo suelto o proyecto" });
    if (!picked) return;
    if (picked.action) picked.action();
    else await vscode.commands.executeCommand(picked.command);
  }

  async function selectPath(key, directory, label) {
    const editor = vscode.window.activeTextEditor;
    const uri = editor && editor.document.uri.scheme === "file" ? editor.document.uri : undefined;
    const folder = uri ? vscode.workspace.getWorkspaceFolder(uri) : undefined;
    const selected = await vscode.window.showOpenDialog({
      canSelectFiles: !directory,
      canSelectFolders: directory,
      canSelectMany: false,
      openLabel: "Seleccionar",
      title: label,
    });
    if (!selected || !selected[0]) return;
    const target = folder ? vscode.ConfigurationTarget.WorkspaceFolder : vscode.ConfigurationTarget.Global;
    try {
      await vscode.workspace.getConfiguration("pyxpic", uri).update(key, selected[0].fsPath, target);
      if (key === "devicesDir") pinoutView.refresh();
      void vscode.window.showInformationMessage(`${label}: ${selected[0].fsPath}${folder ? " (proyecto)" : " (usuario)"}`);
    } catch (error) {
      void vscode.window.showErrorMessage(`No se pudo guardar ${label}: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  context.subscriptions.push(
    vscode.commands.registerCommand("pyxpic.check", () => run("check")),
    vscode.commands.registerCommand("pyxpic.build", () => run("build")),
    vscode.commands.registerCommand("pyxpic.clean", clean),
    vscode.commands.registerCommand("pyxpic.refreshResources", () => resourcesView.measure()),
    vscode.commands.registerCommand("pyxpic.examples", examples),
    vscode.commands.registerCommand("pyxpic.welcome", openWelcome),
    vscode.commands.registerCommand("pyxpic.newProject", newProject),
    vscode.commands.registerCommand("pyxpic.openFile", openFile),
    vscode.commands.registerCommand("pyxpic.openProject", openProject),
    vscode.commands.registerCommand("pyxpic.selectTarget", selectTarget),
    vscode.commands.registerCommand("pyxpic.projectCheck", projectCheck),
    vscode.commands.registerCommand("pyxpic.mode", modeMenu),
    vscode.commands.registerCommand("pyxpic.selectCompiler", () => selectPath("executable", false, "Compilador PyxPic")),
    vscode.commands.registerCommand("pyxpic.selectDevices", () => selectPath("devicesDir", true, "Carpeta de Device Packs")),
    vscode.commands.registerCommand("pyxpic.selectPicAs", () => selectPath("picAsPath", false, "PIC-AS (ruta informativa, no integrada)")),
    vscode.commands.registerCommand("pyxpic.status", () => {
      const document = vscode.window.activeTextEditor && vscode.window.activeTextEditor.document;
      output.clear();
      output.appendLine(`PyxPic IDE ${extensionVersion} — Estado y ayuda\n`);
      if (document && document.uri.scheme === "file") {
        const data = environment(document);
        output.appendLine(`Archivo: ${document.uri.fsPath}\nModo: ${document.languageId}\nDispositivo: ${data.device}\n${data.rows.map((row) => `${row.name}: ${row.value} — ${row.state}`).join("\n")}`);
      } else output.appendLine("Abre un archivo PyxPic para ver su configuración.");
      output.appendLine("\nHerramientas: comprobar, compilar a HEX, abrir ejemplo y configurar PyxPic.\nAl guardar se comprueba el archivo. Escribe ADC. o UART. para ver métodos; coloca el cursor sobre uint16 o ADC para ver ayuda.\nPendiente: recursos de memoria, grabación y depuración.");
      output.show();
    }),
    vscode.commands.registerCommand("pyxpic.settings", () => vscode.commands.executeCommand("workbench.action.openSettings", "@ext:pyxpic.pyxpic-ide")),
  );
}

exports.activate = activate;
