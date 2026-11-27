"use strict";

const fs = require("node:fs");
const path = require("node:path");
const vscodeApi = require("vscode");

/** La versión del informe de `pyxpic memory --format json` que esta vista entiende (docs/memoria.md). */
const REPORT_VERSION = 1;

function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (char) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[char]);
}

function number(value) {
  return Number(value);
}

/** Lee la salida de `pyxpic memory --format json`: el único origen de los números medidos. */
function parseMemoryReport(text) {
  let report;
  try {
    report = JSON.parse(String(text));
  } catch {
    return { ok: false, error: "La CLI no devolvió un informe de memoria (JSON)." };
  }
  if (!report || typeof report.version !== "number") return { ok: false, error: "El informe de memoria no dice su versión." };
  if (report.version !== REPORT_VERSION) return { ok: false, error: `La versión ${report.version} del informe de memoria no la entiende esta extensión (entiende la ${REPORT_VERSION}): actualiza la extensión o la CLI.` };
  const fields = [report.program?.capacity, report.program?.used, report.program?.word_bits, report.ram?.compiler_usable, report.ram?.used, report.ram?.chip_total];
  if (!fields.every(Number.isFinite)) return { ok: false, error: "El informe de memoria está incompleto." };
  return { ok: true, report };
}

/** El mensaje que explica por qué no se pudo medir: la CLI ausente o lo que dijo el compilador. */
function measureFailure(result) {
  if (result?.error && (result.error.code === "ENOENT" || /ENOENT/.test(String(result.error.message)))) {
    return "No se encontró la CLI de PyxPic: configura pyxpic.executable (PyxPic: Seleccionar compilador).";
  }
  const lines = `${result?.stderr || ""}\n${result?.stdout || ""}`.split(/\r?\n/).filter((line) => /error|falta|no se pudo/i.test(line));
  return lines.length ? lines.slice(0, 3).join(" · ") : "La CLI no pudo medir el programa. Consulta la salida PyxPic.";
}

function sourcePeripherals(source) {
  const found = new Set();
  const imports = /^\s*from\s+pyxpic\.periph\.(\w+)\s+import\s+([^\n#]+)/gm;
  for (const match of source.matchAll(imports)) {
    const moduleName = match[1];
    const names = match[2].split(",").map((name) => name.trim());
    if (moduleName === "adc") found.add("ADC");
    if (moduleName === "eeprom") found.add("EEPROM");
    if (moduleName === "uart") found.add("EUSART");
    if (moduleName === "pwm") {
      if (names.includes("PWM1")) found.add("ECCP");
      if (names.includes("PWM2")) found.add("CCP2");
    }
    if (moduleName === "timer") {
      for (const timer of [0, 1, 2]) if (names.some((name) => name.includes(String(timer)))) found.add(`TIMER${timer}`);
    }
  }
  return [...found];
}

/** Las capacidades del dispositivo (Device Pack) y, si hay, lo medido: `state` es { report, stale, error, measuring }. */
function readResources(devicesDir, deviceName, source, state) {
  if (!devicesDir || !deviceName) return { kind: "empty", message: "Abre un archivo PyxPic con un dispositivo o perfil de destino." };
  try {
    const deviceDir = path.join(devicesDir, deviceName.toLowerCase());
    const memory = JSON.parse(fs.readFileSync(path.join(deviceDir, "memory.json"), "utf8"));
    const peripheralData = JSON.parse(fs.readFileSync(path.join(deviceDir, "peripherals.json"), "utf8"));
    const programAvailable = number(memory?.program?.words);
    const ramAvailable = number(memory?.data?.ram_bytes);
    const bankStart = number(memory?.data?.gpr?.[0]?.start);
    const commonStart = number(memory?.data?.common?.start);
    const compilerRamAvailable = commonStart - bankStart;
    const peripherals = Array.isArray(peripheralData?.peripherals) ? peripheralData.peripherals.map((entry) => entry.name) : [];
    if (![programAvailable, ramAvailable, compilerRamAvailable].every(Number.isFinite) || peripherals.length === 0) throw new Error("datos de recursos incompletos");
    // un informe de otro dispositivo (el programa cambió de PIC) no se atribuye a este
    const report = state?.report && String(state.report.device).toLowerCase() === deviceName.toLowerCase() ? state.report : undefined;
    return {
      kind: "ready",
      deviceName,
      programAvailable,
      ramAvailable,
      compilerRamAvailable,
      eepromAvailable: number(memory?.eeprom?.bytes) || 0,
      peripherals,
      usedPeripherals: sourcePeripherals(source || "").filter((name) => peripherals.includes(name)),
      report,
      stale: Boolean(report && state?.stale),
      error: state?.error,
      measuring: Boolean(state?.measuring),
    };
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    return { kind: "empty", message: `No se pudieron leer los recursos de ${deviceName}: ${detail}` };
  }
}

function meter(label, used, available, unit) {
  const percent = available > 0 ? Math.min(100, Math.round((used / available) * 1000) / 10) : 0;
  return `<div class="meter"><div><strong>${escapeHtml(label)}</strong><span>${used} / ${available} ${escapeHtml(unit)} · ${percent}%</span></div><progress max="${available}" value="${used}" aria-label="${escapeHtml(label)}: ${used} de ${available} ${escapeHtml(unit)}"></progress></div>`;
}

const REFRESH = `<a class="action" href="command:pyxpic.refreshResources">Medir ahora</a>`;

function statusHtml(model) {
  const parts = [];
  if (model.measuring) parts.push(`<p class="notice" role="status">Midiendo con la CLI…</p>`);
  if (model.error) parts.push(`<p class="notice error" role="alert"><strong>No se pudo medir.</strong> ${escapeHtml(model.error)}${model.report ? " Se muestran los datos de la última medición correcta." : ""} ${REFRESH}</p>`);
  if (model.report && model.stale && !model.error) parts.push(`<p class="notice stale" role="status"><strong>Desactualizado:</strong> el archivo cambió después de la última medición. ${REFRESH}</p>`);
  if (model.report && model.report.complete === false) {
    const notes = (model.report.notes || []).map((note) => `<li>${escapeHtml(note)}</li>`).join("");
    parts.push(`<div class="notice partial" role="status"><strong>Resultado parcial.</strong> Hay partes que la CLI no puede analizar:<ul>${notes}</ul></div>`);
  }
  if (!model.report && !model.error && !model.measuring) parts.push(`<p class="notice">Todavía no se midió este archivo: se mide al verificarlo o compilarlo. ${REFRESH}</p>`);
  return parts.join("");
}

function reportHtml(model) {
  const r = model.report;
  const wordBits = r.program.word_bits;
  const meters = [meter("Programa (flash)", r.program.used, r.program.capacity, `palabras de ${wordBits} bits`), meter("RAM del compilador", r.ram.used, r.ram.compiler_usable, "bytes")];
  if (r.stack && Number.isFinite(r.stack.worst_case)) meters.push(meter("Pila de hardware", r.stack.worst_case, r.stack.levels, "niveles"));
  const rows = [
    ["RAM total del PIC", `${r.ram.chip_total} bytes`],
    ["Zona común reservada (ABI)", `${r.ram.reserved_common} bytes`],
    ["Dirección más alta de programa", `0x${Math.max(0, r.program.highest_address_used - 1).toString(16).toUpperCase().padStart(4, "0")}`],
    ["Arranque y vectores", `${r.program.startup_and_vectors} palabras`],
    ["Configuración (no cuenta)", `${r.program.config_words} palabras`],
    ["EEPROM", `${model.eepromAvailable} bytes`],
  ];
  const banks = (r.ram.banks || []).map((b) => `<tr><th scope="row">Banco ${b.bank}</th><td>${b.used} / ${b.compiler_capacity}</td><td>${b.gpr_bytes}</td></tr>`).join("");
  return `${meters.join("")}<dl>${rows.map(([k, v]) => `<div><dt>${escapeHtml(k)}</dt><dd>${escapeHtml(v)}</dd></div>`).join("")}</dl>
    <details><summary>RAM por banco</summary><table><thead><tr><th scope="col">Banco</th><th scope="col">Usado / del compilador</th><th scope="col">GPR del chip</th></tr></thead><tbody>${banks}</tbody></table><p>El compilador solo usa el banco 0 por ahora; los demás se muestran como reserva, no como uso.</p></details>`;
}

function readyHtml(model) {
  const r = model.report;
  const memory = r ? `<section class="memory">${reportHtml(model)}</section>` : `<section class="memory"><dl><div><dt>Programa (flash)</dt><dd>${model.programAvailable} palabras</dd></div><div><dt>RAM del compilador</dt><dd>${model.compilerRamAvailable} bytes</dd></div><div><dt>RAM total del PIC</dt><dd>${model.ramAvailable} bytes</dd></div><div><dt>EEPROM</dt><dd>${model.eepromAvailable} bytes</dd></div></dl></section>`;
  const peripherals = r ? r.peripherals || [] : model.usedPeripherals;
  const used = peripherals.length ? peripherals.map((name) => `<span>${escapeHtml(name)}</span>`).join("") : `<em>${r ? "Ninguno." : "Ninguno detectado en los imports."}</em>`;
  const pins = r && (r.pins || []).length ? `<section class="peripherals"><div class="section-title"><h2>Pines usados</h2><span>${r.pins.length}</span></div><div class="chips">${r.pins.map((p) => `<span>${escapeHtml(p)}</span>`).join("")}</div></section>` : "";
  const source = r ? "medidos por la CLI (pyxpic memory)" : "detectados desde los imports del archivo";
  return `<header><h1>${escapeHtml(model.deviceName)}</h1><p>Recursos usados / disponibles</p></header>
    ${statusHtml(model)}${memory}
    <section class="peripherals"><div class="section-title"><h2>Periféricos</h2><span>${peripherals.length} ${r ? "usados" : "referenciados"} / ${model.peripherals.length} disponibles</span></div><div class="chips">${used}</div><details><summary>Ver catálogo disponible</summary><p>${escapeHtml(model.peripherals.join(" · "))}</p></details></section>
    ${pins}
    <p class="footnote">Los periféricos ${source}. La RAM medida corresponde al banco 0, el único que usa el compilador por ahora. Nada se graba en el PIC.</p>`;
}

function html(webview, extensionUri, model) {
  const css = webview.asWebviewUri(vscodeApi.Uri.joinPath(extensionUri, "media", "resources.css"));
  const content = model.kind === "ready" ? readyHtml(model) : `<section class="empty"><h1>Recursos</h1><p>${escapeHtml(model.message)}</p></section>`;
  return `<!doctype html><html lang="es"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource}"><link rel="stylesheet" href="${css}"><title>PyxPic: Recursos</title></head><body><main>${content}</main></body></html>`;
}

/**
 * La vista Recursos. `currentHardware()` dice qué archivo y dispositivo hay; `measure()` ejecuta `pyxpic memory --format json` para el archivo activo y
 * devuelve { error, stdout, stderr }. Cada archivo guarda su última medición: se marca desactualizada cuando el texto cambia y se conserva (con el
 * error) si la siguiente falla.
 */
function register(vscode, context, currentHardware, measure) {
  let view;
  let latestRequest;
  const states = new Map();
  const render = async () => {
    if (!view) return;
    const request = Symbol();
    latestRequest = request;
    const current = await currentHardware();
    if (latestRequest !== request) return;
    const model = readResources(current.devicesDir, current.deviceName, current.source, states.get(current.fileKey));
    view.webview.html = html(view.webview, context.extensionUri, model);
  };
  const provider = {
    resolveWebviewView(webviewView) {
      view = webviewView;
      view.webview.options = { enableCommandUris: ["pyxpic.refreshResources"], localResourceRoots: [vscode.Uri.joinPath(context.extensionUri, "media")] };
      void render();
    },
  };
  async function measureNow() {
    // sin la vista abierta no hay a quien mostrarle la medicion (al abrirla ofrece "Medir ahora")
    if (!view) return;
    const current = await currentHardware();
    const key = current.fileKey;
    if (!key || !measure) return;
    const before = states.get(key);
    states.set(key, { ...before, measuring: true });
    await render();
    const result = await measure();
    const parsed = result?.error && !result.stdout ? { ok: false, error: measureFailure(result) } : parseMemoryReport(result?.stdout);
    if (parsed.ok) states.set(key, { report: parsed.report, stale: false });
    else states.set(key, { report: before?.report, stale: before?.stale, error: parsed.error === "La CLI no devolvió un informe de memoria (JSON)." ? measureFailure(result) : parsed.error });
    await render();
  }
  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider("pyxpic.resources", provider, { webviewOptions: { retainContextWhenHidden: true } }),
    vscode.window.onDidChangeActiveTextEditor(() => { void render(); }),
  );
  return {
    refresh: () => { void render(); },
    measure: measureNow,
    /** El texto del archivo cambió: lo medido ya no corresponde a lo que hay en el editor. */
    markStale(fileKey) {
      const state = states.get(fileKey);
      if (state?.report && !state.stale) {
        states.set(fileKey, { ...state, stale: true });
        void render();
      }
    },
  };
}

module.exports = { REPORT_VERSION, html, measureFailure, parseMemoryReport, readResources, register, sourcePeripherals };
