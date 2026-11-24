"use strict";

const vscode = require("vscode");
const path = require("node:path");
const { execFile } = require("node:child_process");

function activate(context) {
  const output = vscode.window.createOutputChannel("PyPIC");
  context.subscriptions.push(output);

  const actions = [
    { label: "Comprobar archivo", command: "pypic.check", icon: "check" },
    { label: "Compilar a HEX", command: "pypic.build", icon: "tools" },
    { label: "Abrir ejemplo", command: "pypic.examples", icon: "book" },
    { label: "Configurar PyPIC", command: "pypic.settings", icon: "settings-gear" },
  ];
  context.subscriptions.push(vscode.window.registerTreeDataProvider("pypic.actions", {
    getTreeItem: (item) => item,
    getChildren: () => actions.map((action) => {
      const item = new vscode.TreeItem(action.label, vscode.TreeItemCollapsibleState.None);
      item.iconPath = new vscode.ThemeIcon(action.icon);
      item.command = { command: action.command, title: action.label };
      return item;
    }),
  }));

  async function run(kind) {
    if (!vscode.workspace.isTrusted) {
      void vscode.window.showErrorMessage("PyPIC necesita un espacio de trabajo de confianza para ejecutar el compilador.");
      return;
    }
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.uri.scheme !== "file" || editor.document.languageId !== "python") {
      void vscode.window.showErrorMessage("Abre un archivo Python local de PyPIC antes de continuar.");
      return;
    }
    const document = editor.document;
    if (document.isDirty && !(await document.save())) return;
    const file = document.uri.fsPath;
    const args = [kind, file];
    if (kind === "build") {
      const suggested = vscode.Uri.file(path.join(path.dirname(file), `${path.parse(file).name}.hex`));
      const target = await vscode.window.showSaveDialog({ defaultUri: suggested, filters: { "Intel HEX": ["hex"] }, saveLabel: "Compilar" });
      if (!target) return;
      args.push("-o", target.fsPath);
    }
    const config = vscode.workspace.getConfiguration("pypic", document.uri);
    const executable = config.get("executable", "pypic");
    const devices = config.get("devicesDir", "").trim();
    if (devices) args.push("--devices-dir", devices);
    const folder = vscode.workspace.getWorkspaceFolder(document.uri);
    const cwd = folder ? folder.uri.fsPath : path.dirname(file);
    output.clear();
    output.appendLine(`PyPIC ${kind}: ${file}`);
    output.show(true);
    execFile(executable, args, { cwd, maxBuffer: 4 * 1024 * 1024 }, (error, stdout, stderr) => {
      if (stdout) output.append(stdout);
      if (stderr) output.append(stderr);
      if (error) {
        output.appendLine(`\nNo se completó: ${error.message}`);
        void vscode.window.showErrorMessage("PyPIC no pudo completar la operación. Consulta la salida PyPIC y la ruta del ejecutable.");
      } else {
        void vscode.window.showInformationMessage(kind === "build" ? "PyPIC: compilación terminada." : "PyPIC: comprobación terminada.");
      }
    });
  }

  async function examples() {
    const files = await vscode.workspace.findFiles("**/examples/**/*.py", "**/{target,node_modules}/**", 200);
    if (!files.length) {
      void vscode.window.showInformationMessage("No se encontraron ejemplos. Abre la carpeta del repositorio PyPIC.");
      return;
    }
    const items = files.map((uri) => ({ label: vscode.workspace.asRelativePath(uri), uri }));
    const selected = await vscode.window.showQuickPick(items, { placeHolder: "Selecciona un ejemplo PyPIC" });
    if (selected) await vscode.window.showTextDocument(selected.uri);
  }

  context.subscriptions.push(
    vscode.commands.registerCommand("pypic.check", () => run("check")),
    vscode.commands.registerCommand("pypic.build", () => run("build")),
    vscode.commands.registerCommand("pypic.examples", examples),
    vscode.commands.registerCommand("pypic.settings", () => vscode.commands.executeCommand("workbench.action.openSettings", "@ext:pypic.pypic-ide")),
  );
}

exports.activate = activate;
