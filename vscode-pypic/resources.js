"use strict";

const fs = require("node:fs");
const path = require("node:path");
const vscodeApi = require("vscode");

function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (char) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[char]);
}

function number(value) {
  return Number(value);
}

function parseBuildSummary(output) {
  const device = /^\s*Dispositivo:\s*(\S+)\s*$/m.exec(output)?.[1];
  const programUsed = /^\s*Programa:\s*(\d+)\s+palabras\s*$/m.exec(output)?.[1];
  const ramUsed = /^\s*RAM:\s*(\d+)\s+bytes/m.exec(output)?.[1];
  return device && programUsed && ramUsed ? { device, programUsed: number(programUsed), ramUsed: number(ramUsed) } : undefined;
}

function sourcePeripherals(source) {
  const found = new Set();
  const imports = /^\s*from\s+pypic\.periph\.(\w+)\s+import\s+([^\n#]+)/gm;
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

function readResources(devicesDir, deviceName, source, summary) {
  if (!devicesDir || !deviceName) return { kind: "empty", message: "Abre un archivo PyPIC con un dispositivo o perfil de destino." };
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
    const matchingSummary = summary?.device?.toLowerCase() === deviceName.toLowerCase() ? summary : undefined;
    return {
      kind: "ready",
      deviceName,
      programAvailable,
      ramAvailable,
      compilerRamAvailable,
      eepromAvailable: number(memory?.eeprom?.bytes) || 0,
      peripherals,
      usedPeripherals: sourcePeripherals(source || "").filter((name) => peripherals.includes(name)),
      summary: matchingSummary,
    };
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    return { kind: "empty", message: `No se pudieron leer los recursos de ${deviceName}: ${detail}` };
  }
}

function meter(label, used, available, unit) {
  const percent = Math.min(100, Math.round((used / available) * 1000) / 10);
  return `<div class="meter"><div><strong>${escapeHtml(label)}</strong><span>${used} / ${available} ${escapeHtml(unit)} · ${percent}%</span></div><progress max="${available}" value="${used}" aria-label="${escapeHtml(label)}: ${used} de ${available} ${escapeHtml(unit)}"></progress></div>`;
}

function readyHtml(model) {
  const summary = model.summary;
  const memory = summary
    ? `${meter("Programa / ROM", summary.programUsed, model.programAvailable, "palabras")}${meter("RAM del compilador", summary.ramUsed, model.compilerRamAvailable, "bytes")}`
    : `<p class="notice">Compila el archivo para medir el uso de programa y RAM.</p>`;
  const used = model.usedPeripherals.length ? model.usedPeripherals.map((name) => `<span>${escapeHtml(name)}</span>`).join("") : "<em>Ninguno detectado en los imports.</em>";
  return `<header><h1>${escapeHtml(model.deviceName)}</h1><p>Recursos usados / disponibles</p></header>
    <section class="memory">${memory}<dl><div><dt>RAM total del PIC</dt><dd>${model.ramAvailable} bytes</dd></div><div><dt>EEPROM</dt><dd>${model.eepromAvailable} bytes</dd></div></dl></section>
    <section class="peripherals"><div class="section-title"><h2>Periféricos</h2><span>${model.usedPeripherals.length} referenciados / ${model.peripherals.length} disponibles</span></div><div class="chips">${used}</div><details><summary>Ver catálogo disponible</summary><p>${escapeHtml(model.peripherals.join(" · "))}</p></details></section>
    <p class="footnote">La RAM medida corresponde al banco 0 que usa actualmente el compilador. Los periféricos se detectan desde los imports del archivo.</p>`;
}

function html(webview, extensionUri, model) {
  const css = webview.asWebviewUri(vscodeApi.Uri.joinPath(extensionUri, "media", "resources.css"));
  const content = model.kind === "ready" ? readyHtml(model) : `<section class="empty"><h1>Recursos</h1><p>${escapeHtml(model.message)}</p></section>`;
  return `<!doctype html><html lang="es"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource}"><link rel="stylesheet" href="${css}"><title>PyPIC: Recursos</title></head><body><main>${content}</main></body></html>`;
}

function register(vscode, context, currentHardware) {
  let view;
  let latestRequest;
  const summaries = new Map();
  const render = async () => {
    if (!view) return;
    const request = Symbol();
    latestRequest = request;
    const current = await currentHardware();
    if (latestRequest !== request) return;
    const model = readResources(current.devicesDir, current.deviceName, current.source, summaries.get(current.fileKey));
    view.webview.html = html(view.webview, context.extensionUri, model);
  };
  const provider = {
    resolveWebviewView(webviewView) {
      view = webviewView;
      view.webview.options = { localResourceRoots: [vscode.Uri.joinPath(context.extensionUri, "media")] };
      void render();
    },
  };
  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider("pypic.resources", provider, { webviewOptions: { retainContextWhenHidden: true } }),
    vscode.window.onDidChangeActiveTextEditor(() => { void render(); }),
  );
  return {
    refresh: () => { void render(); },
    updateBuild(fileKey, output) {
      const summary = parseBuildSummary(output);
      if (summary) summaries.set(fileKey, summary);
      void render();
    },
  };
}

module.exports = { html, parseBuildSummary, readResources, register, sourcePeripherals };
