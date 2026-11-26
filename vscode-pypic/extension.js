"use strict";

const vscode = require("vscode");
const fs = require("node:fs");
const path = require("node:path");
const { execFile } = require("node:child_process");
const language = require("./language");
const project = require("./project");
const modes = require("./projectmode");
const sidebar = require("./sidebar");
const welcome = require("./welcome");
const pinout = require("./pinout");
const resources = require("./resources");
const extensionVersion = require("./package.json").version;
const { registerSpelling } = require("./spelling");

const welcomeCommands = Object.freeze({
  newProject: "pypic.newProject",
  openFile: "pypic.openFile",
  openProject: "pypic.openProject",
  openExample: "pypic.examples",
  check: "pypic.check",
  selectCompiler: "pypic.selectCompiler",
  selectDevices: "pypic.selectDevices",
  selectPicAs: "pypic.selectPicAs",
});

function activate(context) {
  const output = vscode.window.createOutputChannel("PyPIC");
  const diagnostics = vscode.languages.createDiagnosticCollection("PyPIC");
  const checks = new Map();
  let welcomePanel;
  context.subscriptions.push(output, diagnostics);
  registerSpelling(vscode, context.extensionPath, (message) => output.appendLine(message));

  // Archivo suelto o proyecto (projectmode.js): un archivo abierto con "Abrir archivo" se trata como suelto aunque haya un pypic.toml cerca.
  const standalone = new Set();
  const statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
  statusBar.command = "pypic.mode";
  // botones de la barra de estado: verificar, compilar y limpiar, visibles mientras hay un archivo PyPIC activo
  const buttons = [
    ["$(check) Verificar", "pypic.check", "PyPIC: verificar el código (pypic check)"],
    ["$(tools) Compilar", "pypic.build", "PyPIC: compilar a HEX (pypic build)"],
    ["$(trash) Limpiar", "pypic.clean", "PyPIC: borrar los .hex que PyPIC generó"],
  ].map(([text, command, tooltip], index) => {
    const item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 99 - index);
    Object.assign(item, { text, command, tooltip });
    return item;
  });
  context.subscriptions.push(statusBar, ...buttons);
  const targetKey = (projectFile) => `pypic.target:${projectFile}`;

  function contextFor(document) {
    const file = document.uri.fsPath;
    const projectFile = modes.findProject(file);
    return modes.resolve(file, { standalone: standalone.has(file), target: projectFile && context.workspaceState.get(targetKey(projectFile)) });
  }

  function refreshStatus() {
    const editor = vscode.window.activeTextEditor;
    const document = editor && editor.document;
    if (!document || document.uri.scheme !== "file" || !["python", "pypic"].includes(document.languageId)) {
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

  /** Ejecuta la CLI y devuelve su salida; no lanza: el código de salida distinto de 0 es parte del resultado. */
  function cli(tools, args, cwd) {
    return new Promise((resolve) => {
      execFile(tools.executable, args, { cwd, maxBuffer: 4 * 1024 * 1024 }, (error, stdout, stderr) => resolve({ error, stdout: String(stdout), stderr: String(stderr) }));
    });
  }

  function detect(document) {
    if (document.languageId === "python" && language.isPyPic(document.getText())) {
      void vscode.languages.setTextDocumentLanguage(document, "pypic");
    }
  }
  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument(detect),
    vscode.workspace.onDidChangeTextDocument((event) => { diagnostics.delete(event.document.uri); detect(event.document); }),
    vscode.workspace.onDidCloseTextDocument((document) => { diagnostics.delete(document.uri); checks.delete(document.uri.toString()); }),
    vscode.workspace.onDidSaveTextDocument((document) => {
      if (document.languageId === "pypic" && vscode.workspace.getConfiguration("pypic", document.uri).get("checkOnSave", true)) void run("check", document, true);
      pinoutView.refresh();
    }),
    vscode.languages.registerHoverProvider("pypic", {
      provideHover(document, position) {
        const range = document.getWordRangeAtPosition(position);
        const word = range && document.getText(range);
        return word && Object.hasOwn(language.info, word) ? new vscode.Hover(language.info[word]) : undefined;
      },
    }),
    vscode.languages.registerCompletionItemProvider("pypic", {
      provideCompletionItems(document, position) {
        const prefix = document.lineAt(position.line).text.slice(0, position.character);
        return language.completions(prefix).map((name) => {
          const item = new vscode.CompletionItem(name);
          item.detail = language.info[name] || "PyPIC";
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
  const resourcesView = resources.register(vscode, context, async () => {
    const document = vscode.window.activeTextEditor?.document;
    const hardware = await currentHardware();
    return {
      devicesDir: hardware.devicesDir,
      deviceName: hardware.detected?.name,
      fileKey: document?.uri.toString(),
      source: document?.getText() || "",
    };
  });
  refreshStatus();

  // Limpiar solo borra los .hex que PyPIC dijo haber generado (el resumen de `pypic build`), nunca rutas adivinadas.
  const BUILT = "pypic.builtFiles";
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
      void vscode.window.showInformationMessage("PyPIC: no hay archivos .hex generados que limpiar.");
      return;
    }
    const names = existing.map((file) => path.basename(file)).join(", ");
    const choice = await vscode.window.showWarningMessage(`PyPIC: se borrarán ${existing.length} archivo(s) .hex generados por la compilación: ${names}`, { modal: true, detail: existing.join(String.fromCharCode(10)) }, "Limpiar");
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
    output.appendLine(`PyPIC clean: ${existing.length - failed.length} archivo(s) borrados${failed.length ? `, ${failed.length} con error` : ""}`);
    existing.filter((file) => !failed.some((line) => line.startsWith(`${file}:`))).forEach((file) => output.appendLine(`  borrado ${file}`));
    failed.forEach((line) => output.appendLine(`  no se pudo borrar ${line}`));
    void vscode.window.showInformationMessage(failed.length ? "PyPIC: no se pudieron borrar todos los archivos. Consulta la salida PyPIC." : `PyPIC: limpieza terminada (${existing.length} archivo(s)).`);
  }

  async function run(kind, targetDocument, quiet = false) {
    if (!vscode.workspace.isTrusted) {
      if (!quiet) void vscode.window.showErrorMessage("PyPIC necesita un espacio de trabajo de confianza para ejecutar el compilador.");
      return;
    }
    const editor = vscode.window.activeTextEditor;
    const document = targetDocument || (editor && editor.document);
    if (!document || document.uri.scheme !== "file" || !["python", "pypic"].includes(document.languageId)) {
      void vscode.window.showErrorMessage("Abre un archivo Python local de PyPIC antes de continuar.");
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
    const config = vscode.workspace.getConfiguration("pypic", document.uri);
    const folder = vscode.workspace.getWorkspaceFolder(document.uri);
    const cwd = ctx.mode === "project" ? path.dirname(ctx.projectFile) : folder ? folder.uri.fsPath : path.dirname(file);
    const tools = language.projectTools(file, folder ? folder.uri.fsPath : path.dirname(file), config.get("executable", "pypic"), config.get("devicesDir", "").trim());
    const args = modes.invocation(kind, ctx, { output: saveTo, devices: tools.devices });
    const key = document.uri.toString();
    const request = Symbol();
    checks.set(key, request);
    output.clear();
    output.appendLine(`PyPIC ${kind}: ${file}`);
    output.appendLine(ctx.mode === "project" ? `Proyecto: ${ctx.projectFile}${ctx.target ? ` (perfil ${ctx.target})` : ""}` : "Archivo suelto");
    // al comprobar o compilar se abre y se selecciona el panel Salida (canal PyPIC) sin quitar el foco del editor
    if (!quiet) output.show(true);
    execFile(tools.executable, args, { cwd, maxBuffer: 4 * 1024 * 1024 }, (error, stdout, stderr) => {
      if (stdout) output.append(stdout);
      if (stderr) output.append(stderr);
      if (!error && kind === "build") resourcesView.updateBuild(key, stdout);
      if (!quiet) output.show(true);
      if (checks.get(key) === request && document.version === version && !document.isClosed) {
        const entries = language.diagnostics(`${stdout}\n${stderr}`, file).map((item) => {
          const range = new vscode.Range(item.line, item.column, item.line, item.column + 1);
          const severity = item.severity === "error" ? vscode.DiagnosticSeverity.Error : vscode.DiagnosticSeverity.Warning;
          const diagnostic = new vscode.Diagnostic(range, item.message, severity);
          diagnostic.code = item.code;
          diagnostic.source = "PyPIC";
          return diagnostic;
        });
        diagnostics.set(document.uri, entries);
      }
      if (error) {
        output.appendLine(`\nNo se completó: ${error.message}`);
        if (!quiet) void vscode.window.showErrorMessage("PyPIC no pudo completar la operación. Consulta la salida PyPIC y la ruta del ejecutable.");
      } else if (!quiet) {
        const hex = kind === "build" ? modes.outputFile(stdout) : undefined;
        if (hex) void rememberBuilt(hex);
        void vscode.window.showInformationMessage(kind === "build" ? `PyPIC: compilación terminada${hex ? `: ${hex}` : ""}.` : "PyPIC: comprobación terminada.");
      }
    });
  }

  async function examples() {
    const files = await vscode.workspace.findFiles("**/examples/**/*.{py,bas,s}", "**/{target,node_modules}/**", 200);
    if (!files.length) {
      void vscode.window.showInformationMessage("No se encontraron ejemplos. Abre la carpeta del repositorio PyPIC.");
      return;
    }
    const items = files.map((uri) => ({ label: vscode.workspace.asRelativePath(uri), uri }));
    const selected = await vscode.window.showQuickPick(items, { placeHolder: "Selecciona un ejemplo PyPIC" });
    if (selected) await vscode.window.showTextDocument(selected.uri);
  }

  function environment(document) {
    const uri = document && document.uri.scheme === "file" ? document.uri : undefined;
    const folder = uri && vscode.workspace.getWorkspaceFolder(uri);
    const cwd = folder ? folder.uri.fsPath : uri ? path.dirname(uri.fsPath) : vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || process.cwd();
    const config = vscode.workspace.getConfiguration("pypic", uri);
    const tools = language.projectTools(uri?.fsPath || cwd, cwd, config.get("executable", "pypic"), config.get("devicesDir", "").trim());
    const exists = (value) => value && require("node:fs").existsSync(value);
    const packsReady = tools.devices && exists(path.join(tools.devices, "pic16f887", "device.json"));
    const picAs = config.get("picAsPath", "");
    return {
      tools,
      rows: [
        { name: "Compilador PyPIC", value: tools.executable, detail: exists(tools.executable) ? "Ejecutable encontrado y listo para comprobar o compilar." : "Selecciona el ejecutable compilado o instala PyPIC en PATH.", state: exists(tools.executable) ? "Listo" : "Falta configurar", kind: exists(tools.executable) ? "ready" : "warning", action: "Elegir compilador", command: "selectCompiler" },
        { name: "Device Packs", value: tools.devices || "Búsqueda predeterminada", detail: packsReady ? "PIC16F887 está disponible para el proyecto." : "Selecciona la carpeta devices que contiene los Device Packs.", state: packsReady ? "Listo" : "Falta configurar", kind: packsReady ? "ready" : "warning", action: "Elegir carpeta", command: "selectDevices" },
        { name: "PIC-AS de XC8", value: picAs || "Sin configurar", detail: picAs ? "La ruta está registrada. La ejecución de PIC-AS desde la extensión sigue pendiente." : "Opcional: PyPIC Native no necesita XC8 ni PIC-AS.", state: picAs ? "Registrado" : "Opcional", kind: picAs ? "ready" : "warning", action: "Elegir PIC-AS", command: "selectPicAs" },
        { name: "Espacio de trabajo", value: vscode.workspace.isTrusted ? "Confiable" : "Modo restringido", detail: vscode.workspace.isTrusted ? "La extensión puede compilar y crear proyectos." : "Confía en la carpeta para habilitar compilación y creación de proyectos.", state: vscode.workspace.isTrusted ? "Listo" : "Requiere confianza", kind: vscode.workspace.isTrusted ? "ready" : "warning", action: "", command: "" },
      ],
      projectText: folder ? `Carpeta actual: ${folder.name}.` : "Elige una carpeta de proyecto o crea una nueva para empezar.",
      device: document ? language.declaredDevice(document.getText()) || "No se detectó un dispositivo declarado en el archivo activo." : "Abre un archivo PyPIC para detectar el dispositivo.",
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
    welcomePanel = vscode.window.createWebviewPanel("pypic.welcome", "PyPIC: Inicio", vscode.ViewColumn.Active, { enableScripts: true, localResourceRoots: [vscode.Uri.joinPath(context.extensionUri, "media")] });
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
      void vscode.window.showErrorMessage("PyPIC necesita un espacio de trabajo de confianza para crear un proyecto.");
      return;
    }
    const { tools } = toolsFor(vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || process.cwd());
    // un proyecto de verdad: `pypic init` escribe pypic.toml (dispositivo, reloj y salida) y un main.py de partida valido
    const init = async (dir, name, device, clock) => {
      const result = await cli(tools, ["init", dir, "--name", name, "--device", device, "--clock", clock, ...(tools.devices ? ["--devices-dir", tools.devices] : [])], dir);
      output.clear();
      output.append(`${result.stdout}${result.stderr}`);
      if (result.error) {
        output.show(true);
        throw new Error(result.stderr.trim().split("\n")[0] || "pypic init fallo");
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
    const config = vscode.workspace.getConfiguration("pypic");
    return { tools: language.projectTools(fsPath, cwd, config.get("executable", "pypic"), config.get("devicesDir", "").trim()), cwd };
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

  /** `pypic project check --format json`: el informe de los perfiles (sale con 1 si alguno tiene errores y aun asi imprime el JSON). */
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
    const picked = await vscode.window.showOpenDialog({ canSelectFiles: true, canSelectFolders: false, canSelectMany: false, filters: { "Programa PyPIC (Python)": ["py"] }, openLabel: "Abrir archivo", title: "Abrir archivo PyPIC" });
    if (!picked || !picked[0]) return;
    standalone.add(picked[0].fsPath);
    await vscode.window.showTextDocument(picked[0]);
    refreshStatus();
    void vscode.window.showInformationMessage("Archivo suelto: al compilar se pregunta dónde guardar el .hex. Para usar un proyecto, elige Abrir proyecto.");
  }

  async function openProject() {
    const picked = await vscode.window.showOpenDialog({ canSelectFiles: false, canSelectFolders: true, canSelectMany: false, openLabel: "Abrir proyecto", title: "Abrir proyecto PyPIC (la carpeta que contiene pypic.toml)" });
    if (!picked || !picked[0]) return;
    const folder = picked[0].fsPath;
    const projectFile = path.join(folder, modes.PROJECT_FILE);
    if (!require("node:fs").existsSync(projectFile)) {
      const action = await vscode.window.showErrorMessage(`No hay ${modes.PROJECT_FILE} en ${folder}: no es un proyecto PyPIC.`, "Crear proyecto aquí");
      if (action !== "Crear proyecto aquí") return;
      const { tools } = toolsFor(folder);
      const created = await cli(tools, ["init", folder, ...(tools.devices ? ["--devices-dir", tools.devices] : [])], folder);
      output.clear();
      output.append(`${created.stdout}${created.stderr}`);
      output.show(true);
      if (created.error) {
        void vscode.window.showErrorMessage("PyPIC no pudo crear el proyecto. Consulta la salida PyPIC.");
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
      void vscode.window.showInformationMessage("No hay un proyecto PyPIC abierto: elige Abrir proyecto o Crear proyecto.");
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
      void vscode.window.showInformationMessage("El proyecto no define perfiles de destino. Crea uno con: pypic project set --target NOMBRE --create device=PIC16F887");
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
      void vscode.window.showInformationMessage("No hay un proyecto PyPIC abierto: elige Abrir proyecto o Crear proyecto.");
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
    void (result.error ? vscode.window.showErrorMessage("PyPIC: el proyecto tiene errores. Consulta la salida PyPIC.") : vscode.window.showInformationMessage("PyPIC: proyecto revisado, sin errores."));
  }

  async function modeMenu() {
    const editor = vscode.window.activeTextEditor;
    const document = editor && editor.document;
    const items = [];
    if (document && document.uri.scheme === "file") {
      const ctx = contextFor(document);
      if (ctx.mode === "project") {
        items.push({ label: "$(target) Cambiar perfil de destino", command: "pypic.selectTarget" });
        items.push({ label: "$(file-code) Tratar este archivo como archivo suelto", action: () => { standalone.add(document.uri.fsPath); refreshStatus(); } });
      } else if (modes.findProject(document.uri.fsPath)) {
        items.push({ label: "$(circuit-board) Usar el proyecto de este archivo", action: () => { standalone.delete(document.uri.fsPath); refreshStatus(); } });
      }
    }
    items.push({ label: "$(file) Abrir archivo…", command: "pypic.openFile" }, { label: "$(folder-opened) Abrir proyecto…", command: "pypic.openProject" }, { label: "$(new-file) Crear proyecto…", command: "pypic.newProject" });
    const picked = await vscode.window.showQuickPick(items, { placeHolder: "PyPIC: archivo suelto o proyecto" });
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
      await vscode.workspace.getConfiguration("pypic", uri).update(key, selected[0].fsPath, target);
      if (key === "devicesDir") pinoutView.refresh();
      void vscode.window.showInformationMessage(`${label}: ${selected[0].fsPath}${folder ? " (proyecto)" : " (usuario)"}`);
    } catch (error) {
      void vscode.window.showErrorMessage(`No se pudo guardar ${label}: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  context.subscriptions.push(
    vscode.commands.registerCommand("pypic.check", () => run("check")),
    vscode.commands.registerCommand("pypic.build", () => run("build")),
    vscode.commands.registerCommand("pypic.clean", clean),
    vscode.commands.registerCommand("pypic.examples", examples),
    vscode.commands.registerCommand("pypic.welcome", openWelcome),
    vscode.commands.registerCommand("pypic.newProject", newProject),
    vscode.commands.registerCommand("pypic.openFile", openFile),
    vscode.commands.registerCommand("pypic.openProject", openProject),
    vscode.commands.registerCommand("pypic.selectTarget", selectTarget),
    vscode.commands.registerCommand("pypic.projectCheck", projectCheck),
    vscode.commands.registerCommand("pypic.mode", modeMenu),
    vscode.commands.registerCommand("pypic.selectCompiler", () => selectPath("executable", false, "Compilador PyPIC")),
    vscode.commands.registerCommand("pypic.selectDevices", () => selectPath("devicesDir", true, "Carpeta de Device Packs")),
    vscode.commands.registerCommand("pypic.selectPicAs", () => selectPath("picAsPath", false, "PIC-AS (ruta informativa, no integrada)")),
    vscode.commands.registerCommand("pypic.status", () => {
      const document = vscode.window.activeTextEditor && vscode.window.activeTextEditor.document;
      output.clear();
      output.appendLine(`PyPIC IDE ${extensionVersion} — Estado y ayuda\n`);
      if (document && document.uri.scheme === "file") {
        const data = environment(document);
        output.appendLine(`Archivo: ${document.uri.fsPath}\nModo: ${document.languageId}\nDispositivo: ${data.device}\n${data.rows.map((row) => `${row.name}: ${row.value} — ${row.state}`).join("\n")}`);
      } else output.appendLine("Abre un archivo PyPIC para ver su configuración.");
      output.appendLine("\nHerramientas: comprobar, compilar a HEX, abrir ejemplo y configurar PyPIC.\nAl guardar se comprueba el archivo. Escribe ADC. o UART. para ver métodos; coloca el cursor sobre uint16 o ADC para ver ayuda.\nPendiente: recursos de memoria, grabación y depuración.");
      output.show();
    }),
    vscode.commands.registerCommand("pypic.settings", () => vscode.commands.executeCommand("workbench.action.openSettings", "@ext:pypic.pypic-ide")),
  );
}

exports.activate = activate;
