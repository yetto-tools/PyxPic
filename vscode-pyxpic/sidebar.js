"use strict";

const sections = [
  {
    label: "Proyecto",
    icon: "folder",
    actions: [
    { label: "Inicio de PyxPic", command: "pyxpic.welcome", icon: "home" },
    { label: "Crear proyecto", command: "pyxpic.newProject", icon: "new-file" },
    { label: "Abrir proyecto…", command: "pyxpic.openProject", icon: "folder-opened" },
    { label: "Abrir archivo…", command: "pyxpic.openFile", icon: "file" },
    { label: "Abrir ejemplo", command: "pyxpic.examples", icon: "book" },
    ],
  },
  {
    label: "Construir",
    icon: "tools",
    actions: [
    { label: "Verificar código", command: "pyxpic.check", icon: "check" },
    { label: "Compilar a HEX", command: "pyxpic.build", icon: "tools" },
    { label: "Limpiar compilación", command: "pyxpic.clean", icon: "trash" },
    { label: "Perfil de destino", command: "pyxpic.selectTarget", icon: "target" },
    { label: "Revisar proyecto", command: "pyxpic.projectCheck", icon: "checklist" },
    { label: "Estado y ayuda", command: "pyxpic.status", icon: "info" },
    ],
  },
  {
    label: "Entorno",
    icon: "settings-gear",
    actions: [
    { label: "Seleccionar compilador", command: "pyxpic.selectCompiler", icon: "file-binary" },
    { label: "Seleccionar Device Packs", command: "pyxpic.selectDevices", icon: "folder-opened" },
    { label: "Seleccionar PIC-AS (opcional)", command: "pyxpic.selectPicAs", icon: "file-code" },
    { label: "Configurar PyxPic", command: "pyxpic.settings", icon: "settings-gear" },
    ],
  },
];

function item(vscode, action) {
  const isSection = Object.hasOwn(action, "actions");
  const treeItem = new vscode.TreeItem(action.label, isSection ? vscode.TreeItemCollapsibleState.Collapsed : vscode.TreeItemCollapsibleState.None);
  treeItem.iconPath = new vscode.ThemeIcon(action.icon);
  if (!isSection) treeItem.command = { command: action.command, title: action.label };
  return treeItem;
}

function provider(vscode) {
  return {
    getTreeItem: (action) => item(vscode, action),
    getChildren: (action) => action ? action.actions : sections,
  };
}

function register(vscode, context) {
  context.subscriptions.push(vscode.window.registerTreeDataProvider("pyxpic.actions", provider(vscode)));
}

module.exports = { register, sections };
