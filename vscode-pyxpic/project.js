"use strict";

const path = require("node:path");

const clocks = [
  { label: "Oscilador interno · 4 MHz", value: "Internal 4MHz", detail: "Configuración inicial del PIC16F887; no requiere componentes externos." },
  { label: "Cristal · 20 MHz", value: "Crystal 20MHz", detail: "Cristal o resonador en OSC1/OSC2; reserva RA6 y RA7." },
];

/** `Crystal 20MHz` -> `CLOCK: Crystal` y `FREQUENCY: 20MHz` (dos lineas, como en el fuente). */
function clockLines(clock) {
  const [kind, freq, mode] = String(clock).trim().split(/\s+/);
  return [`CLOCK: ${kind}`, freq ? `FREQUENCY: ${freq}` : undefined, mode ? `OSC_MODE: ${mode}` : undefined].filter(Boolean).join("\n");
}

function source(device, clock) {
  return `"""Proyecto PyxPic para ${device}.\n\nEl firmware se compila de Python a instrucciones nativas del PIC.\n"""\nfrom pyxpic.device import ${device}\nfrom pyxpic.periph.gpio import RD0\nfrom pyxpic.system.time import pause\n\n${clockLines(clock)}\n\n\ndef main() -> None:\n    RD0.output()\n    while True:\n        RD0.toggle()\n        pause(500)\n`;
}

async function create(vscode, init) {
  const name = await vscode.window.showInputBox({
    title: "Crear proyecto PyxPic",
    prompt: "Nombre del proyecto",
    validateInput: (value) => /^[A-Za-z][A-Za-z0-9_-]*$/.test(value) ? undefined : "Usa letras, números, _ o -; debe empezar con una letra.",
  });
  if (!name) return undefined;
  const location = await vscode.window.showOpenDialog({ canSelectFiles: false, canSelectFolders: true, canSelectMany: false, openLabel: "Elegir carpeta" });
  if (!location || !location[0]) return undefined;
  const device = await vscode.window.showQuickPick([{ label: "PIC16F887", detail: "Primer dispositivo soportado por PyxPic." }], { title: "Dispositivo objetivo", placeHolder: "Selecciona un dispositivo" });
  if (!device) return undefined;
  const clock = await vscode.window.showQuickPick(clocks, { title: "Reloj del proyecto", placeHolder: "Selecciona la fuente y frecuencia del reloj" });
  if (!clock) return undefined;

  const root = vscode.Uri.joinPath(location[0], name);
  try {
    await vscode.workspace.fs.stat(root);
    void vscode.window.showErrorMessage(`No se creó el proyecto: ya existe ${root.fsPath}.`);
    return undefined;
  } catch {
    // La ausencia es la precondición para crear un proyecto nuevo.
  }
  await vscode.workspace.fs.createDirectory(root);
  const main = vscode.Uri.joinPath(root, "main.py");
  if (init) {
    // la CLI escribe pyxpic.toml y main.py (un proyecto, no un archivo suelto)
    try {
      await init(root.fsPath, name, device.label, clock.value);
    } catch (error) {
      void vscode.window.showErrorMessage(`No se creó el proyecto: ${error instanceof Error ? error.message : String(error)}`);
      return undefined;
    }
  } else {
    await vscode.workspace.fs.writeFile(main, Buffer.from(source(device.label, clock.value), "utf8"));
  }
  return { root, main, device: device.label, clock: clock.value };
}

function projectName(uri) {
  return path.basename(uri.fsPath);
}

module.exports = { clocks, source, create, projectName };
