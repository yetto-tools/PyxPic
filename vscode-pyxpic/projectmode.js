"use strict";

// Archivo suelto o proyecto. Sin dependencia de VS Code: solo decide el modo, arma los argumentos de la CLI y lee su informe.
//
// * Archivo suelto: funciona como siempre: `pyxpic build archivo.py -o salida.hex` y al compilar se pregunta dónde guardar.
// * Proyecto: hay un `pyxpic.toml` (en la carpeta del archivo o en las de arriba). El dispositivo, el reloj y la salida salen del perfil
//   de destino: `pyxpic build --project pyxpic.toml [--target perfil]`, sin preguntar nada.

const fs = require("node:fs");
const path = require("node:path");

const PROJECT_FILE = "pyxpic.toml";

/** El `pyxpic.toml` más cercano subiendo desde `start` (un archivo o una carpeta); `undefined` si no hay. */
function findProject(start, exists = fs.existsSync) {
  let dir = path.resolve(start);
  try {
    if (!fs.statSync(dir).isDirectory()) dir = path.dirname(dir);
  } catch {
    dir = path.dirname(dir);
  }
  for (;;) {
    const candidate = path.join(dir, PROJECT_FILE);
    if (exists(candidate)) return candidate;
    const parent = path.dirname(dir);
    if (parent === dir) return undefined;
    dir = parent;
  }
}

/** El modo de un archivo: "project" si hay `pyxpic.toml` y no se abrió como archivo suelto; si no, "file". */
function resolve(file, { standalone = false, target, exists } = {}) {
  if (standalone) return { mode: "file", file };
  const projectFile = findProject(file, exists);
  return projectFile ? { mode: "project", file, projectFile, target } : { mode: "file", file };
}

/**
 * Los argumentos de la CLI para `check` o `build`.
 * Archivo suelto: `build` necesita la salida (`output`); Proyecto: la salida es la del perfil y no se pasa nada.
 * `devices` solo se añade si se conoce; en un proyecto, `devices_dir` del `pyxpic.toml` manda si no se pasa.
 */
function invocation(kind, ctx, { output, devices } = {}) {
  const args = [kind];
  if (ctx.mode === "project") {
    if (kind === "check") args.push(ctx.file); // se comprueba ESTE archivo con el dispositivo y el reloj del perfil
    args.push("--project", ctx.projectFile);
    if (ctx.target) args.push("--target", ctx.target);
  } else {
    args.push(ctx.file);
    if (kind === "build") {
      if (!output) throw new Error("un archivo suelto necesita la ruta de salida");
      args.push("-o", output);
    }
  }
  if (devices) args.push("--devices-dir", devices);
  return args;
}

/** La ruta del .hex que dice el resumen de `pyxpic build` ("  Archivo:       ruta"). */
function outputFile(stdout) {
  const match = /^\s*Archivo:\s+(.+?)\s*$/m.exec(String(stdout));
  return match ? match[1] : undefined;
}

/** El informe JSON de `pyxpic project check --format json`; lanza si no es del formato esperado. */
function parseReport(text) {
  const report = JSON.parse(text);
  if (report.version !== 1 || !Array.isArray(report.targets)) throw new Error("informe de proyecto desconocido (versión " + report.version + ")");
  return {
    ok: report.ok === true,
    name: report.name || undefined,
    main: report.main || undefined,
    defaultTarget: report.default_target || undefined,
    targets: report.targets.map((t) => ({
      name: t.name,
      ok: t.status === "ok",
      device: t.device || undefined,
      clock: (t.clock && (t.clock.description || t.clock.declaration)) || undefined,
      errors: t.errors || [],
      warnings: t.warnings || [],
    })),
  };
}

/** Una línea para elegir un perfil: `✓ cristal · PIC16F887 · Crystal 20 MHz (FOSC = HS)`. */
function targetLabel(t) {
  return { label: `${t.ok ? "$(check)" : "$(error)"} ${t.name}`, description: [t.device, t.clock].filter(Boolean).join(" · "), detail: t.ok ? undefined : t.errors[0], target: t.name };
}

/** El texto de la barra de estado: dice si se trabaja con un archivo suelto o con un proyecto. */
function statusText(ctx, name) {
  if (ctx.mode === "project") return `$(circuit-board) PyxPic · proyecto${name ? ` ${name}` : ""}${ctx.target ? ` · ${ctx.target}` : ""}`;
  return "$(file-code) PyxPic · archivo suelto";
}

function statusTooltip(ctx) {
  return ctx.mode === "project"
    ? `Proyecto: ${ctx.projectFile}\nCompilar usa el perfil de destino del proyecto y no pregunta dónde guardar. Clic para cambiar de perfil o tratar este archivo como suelto.`
    : "Archivo suelto: al compilar se pregunta dónde guardar el .hex. Clic para abrir un archivo o un proyecto.";
}

module.exports = { PROJECT_FILE, findProject, resolve, invocation, outputFile, parseReport, targetLabel, statusText, statusTooltip };
