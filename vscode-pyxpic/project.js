"use strict";

const path = require("node:path");

/**
 * Los dos pasos del asistente: PIC y reloj. Los datos salen de `pyxpic devices --format json`, asi que un Device Pack nuevo aparece solo.
 * Devuelve `{ device, clock }` o `undefined` si se cancela.
 */
async function chooseTarget(vscode, devices, current) {
  const items = devices.filter((d) => d.buildable).map((d) => ({
    label: d.name,
    description: `${d.name === current ? "actual · " : ""}${d.program_words} palabras · ${d.ram_bytes} B de RAM · ${d.io_pins} pines de E/S`,
    detail: d.limits ? `Limitado: ${d.limits}` : undefined,
    data: d,
  }));
  if (!items.length) {
    void vscode.window.showErrorMessage("PyxPic no encontró Device Packs para compilar. Usa «PyxPic: Seleccionar Device Packs».");
    return undefined;
  }
  const device = await vscode.window.showQuickPick(items, { title: "PyxPic · Paso 1 de 2: PIC", placeHolder: "¿Para qué PIC vas a compilar?" });
  if (!device) return undefined;
  const clocks = device.data.clocks.map((c) => ({ label: c.label, detail: c.detail, value: c.value }));
  const clock = await vscode.window.showQuickPick(clocks, { title: "PyxPic · Paso 2 de 2: reloj", placeHolder: `Reloj del ${device.label}` });
  if (!clock) return undefined;
  return { device: device.label, clock: clock.value };
}

/** `Crystal 20MHz` -> `CLOCK: Crystal` y `FREQUENCY: 20MHz` (dos lineas, como en el fuente). */
function clockLines(clock) {
  const [kind, freq, mode] = String(clock).trim().split(/\s+/);
  return [`CLOCK: ${kind}`, freq ? `FREQUENCY: ${freq}` : undefined, mode ? `OSC_MODE: ${mode}` : undefined].filter(Boolean).join("\n");
}

function source(device, clock) {
  return `"""Proyecto PyxPic para ${device}.\n\nEl firmware se compila de Python a instrucciones nativas del PIC.\n"""\nfrom pyxpic.device import ${device}\nfrom pyxpic.periph.gpio import RB0\nfrom pyxpic.system.time import pause\n\n${clockLines(clock)}\n\n\ndef main() -> None:\n    RB0.output()\n    while True:\n        RB0.toggle()\n        pause(500)\n`;
}

async function create(vscode, init, devices) {
  const name = await vscode.window.showInputBox({
    title: "Crear proyecto PyxPic",
    prompt: "Nombre del proyecto",
    validateInput: (value) => /^[A-Za-z][A-Za-z0-9_-]*$/.test(value) ? undefined : "Usa letras, números, _ o -; debe empezar con una letra.",
  });
  if (!name) return undefined;
  const location = await vscode.window.showOpenDialog({ canSelectFiles: false, canSelectFolders: true, canSelectMany: false, openLabel: "Elegir carpeta" });
  if (!location || !location[0]) return undefined;
  const target = await chooseTarget(vscode, devices);
  if (!target) return undefined;

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
      await init(root.fsPath, name, target.device, target.clock);
    } catch (error) {
      void vscode.window.showErrorMessage(`No se creó el proyecto: ${error instanceof Error ? error.message : String(error)}`);
      return undefined;
    }
  } else {
    await vscode.workspace.fs.writeFile(main, Buffer.from(source(target.device, target.clock), "utf8"));
  }
  return { root, main, device: target.device, clock: target.clock };
}

function projectName(uri) {
  return path.basename(uri.fsPath);
}

module.exports = { source, create, chooseTarget, projectName };
