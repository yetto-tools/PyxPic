"use strict";

const sections = [
  {
    label: "Proyecto",
    icon: "folder",
    actions: [
    { label: "Inicio de PyPIC", command: "pypic.welcome", icon: "home" },
    { label: "Crear proyecto", command: "pypic.newProject", icon: "new-file" },
    { label: "Abrir proyecto…", command: "pypic.openProject", icon: "folder-opened" },
    { label: "Abrir archivo…", command: "pypic.openFile", icon: "file" },
    { label: "Abrir ejemplo", command: "pypic.examples", icon: "book" },
    ],
  },
  {
    label: "Construir",
    icon: "tools",
    actions: [
    { label: "Verificar código", command: "pypic.check", icon: "check" },
    { label: "Compilar a HEX", command: "pypic.build", icon: "tools" },
    { label: "Perfil de destino", command: "pypic.selectTarget", icon: "target" },
    { label: "Revisar proyecto", command: "pypic.projectCheck", icon: "checklist" },
    { label: "Estado y ayuda", command: "pypic.status", icon: "info" },
    ],
  },
  {
    label: "Entorno",
    icon: "settings-gear",
    actions: [
    { label: "Seleccionar compilador", command: "pypic.selectCompiler", icon: "file-binary" },
    { label: "Seleccionar Device Packs", command: "pypic.selectDevices", icon: "folder-opened" },
    { label: "Seleccionar PIC-AS (opcional)", command: "pypic.selectPicAs", icon: "file-code" },
    { label: "Configurar PyPIC", command: "pypic.settings", icon: "settings-gear" },
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
  context.subscriptions.push(vscode.window.registerTreeDataProvider("pypic.actions", provider(vscode)));
}

module.exports = { register, sections };
