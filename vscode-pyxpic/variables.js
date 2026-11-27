"use strict";

const vscodeApi = require("vscode");

/** La versión del informe de `pyxpic vars --format json` que esta vista entiende. */
const REPORT_VERSION = 1;

function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (char) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[char]);
}

/** Lee la salida de `pyxpic vars --format json`: el compilador es el único origen de la lista. */
function parseVariablesReport(text) {
  let report;
  try {
    report = JSON.parse(String(text));
  } catch {
    return { ok: false, error: "La CLI no devolvió la lista de variables (JSON)." };
  }
  if (!report || typeof report.version !== "number") return { ok: false, error: "El informe de variables no dice su versión." };
  if (report.version !== REPORT_VERSION) return { ok: false, error: `La versión ${report.version} del informe de variables no la entiende esta extensión (entiende la ${REPORT_VERSION}): actualiza la extensión o la CLI.` };
  if (!Array.isArray(report.variables)) return { ok: false, error: "El informe de variables está incompleto." };
  return { ok: true, report };
}

/** Separa la lista en lo que declara el programa, lo predefinido que usa y lo predefinido sin usar. */
function groupVariables(report) {
  const all = report.variables || [];
  const declared = all.filter((v) => v.kind !== "predefined");
  const predefined = all.filter((v) => v.kind === "predefined");
  return { declared, usedPredefined: predefined.filter((v) => v.used), freePredefined: predefined.filter((v) => !v.used) };
}

function lineLink(variable) {
  if (!Number.isFinite(variable.line)) return escapeHtml(variable.name);
  const args = encodeURIComponent(JSON.stringify([variable.line]));
  return `<a href="command:pyxpic.revealLine?${args}" title="Ir a la línea ${variable.line}">${escapeHtml(variable.name)}</a>`;
}

function nameOf(variable) {
  const base = lineLink(variable);
  return Number.isFinite(variable.length) ? `${base}(${variable.length})` : base;
}

function useBadge(variable) {
  return variable.used ? `<span class="on">usada</span>` : `<span class="off">sin usar</span>`;
}

function declaredRows(variables) {
  return variables
    .map((v) => `<tr><th scope="row">${nameOf(v)}</th><td>${escapeHtml(v.kind === "symbol" ? "alias" : v.type)}</td><td>${Number.isFinite(v.line) ? v.line : "-"}</td><td>${useBadge(v)}</td><td>${escapeHtml(v.detail || "")}</td></tr>`)
    .join("");
}

/** El contenido de la vista según el modelo: `empty` (sin archivo BASIC), `error` o `ready`. */
function readyHtml(model) {
  const g = groupVariables(model.report);
  const stale = model.stale ? `<p class="notice stale" role="status"><strong>Desactualizado:</strong> el archivo cambió después de leer la lista.</p>` : "";
  const declared = g.declared.length
    ? `<section><div class="section-title"><h2>Declaradas</h2><span>${g.declared.length}</span></div><table><thead><tr><th scope="col">Nombre</th><th scope="col">Tipo</th><th scope="col">Línea</th><th scope="col">Uso</th><th scope="col">Valor</th></tr></thead><tbody>${declaredRows(g.declared)}</tbody></table></section>`
    : `<section><div class="section-title"><h2>Declaradas</h2><span>0</span></div><p class="muted">El programa no declara variables con <code>DIM</code> ni alias con <code>SYMBOL</code>.</p></section>`;
  const usedRows = g.usedPredefined.map((v) => `<tr><th scope="row">${escapeHtml(v.name)}</th><td>${escapeHtml(v.type)}</td></tr>`).join("");
  const used = g.usedPredefined.length
    ? `<table><thead><tr><th scope="col">Nombre</th><th scope="col">Tipo</th></tr></thead><tbody>${usedRows}</tbody></table>`
    : `<p class="muted">El programa no usa ninguna.</p>`;
  const free = g.freePredefined.map((v) => `<span>${escapeHtml(v.name)}</span>`).join("");
  return `<header><h1>Variables</h1><p>${escapeHtml(model.file)}${model.report.device ? ` · ${escapeHtml(model.report.device)}` : ""}</p></header>
    ${stale}${declared}
    <section><div class="section-title"><h2>Predefinidas en uso</h2><span>${g.usedPredefined.length} / ${g.usedPredefined.length + g.freePredefined.length}</span></div>${used}
    <details><summary>Disponibles sin usar (${g.freePredefined.length})</summary><div class="chips">${free}</div></details></section>
    <p class="footnote"><code>b0</code>..<code>b27</code> son BYTE (0 a 255) y <code>w0</code>..<code>w13</code> son WORD (0 a 65535); existen sin declarar. A diferencia de PICAXE no se solapan (<code>w0</code> no es <code>b0:b1</code>): usar los dos a la vez es un error. Cada variable ocupa su tamaño en el banco 0 de RAM solo si el programa la usa.</p>`;
}

function html(webview, extensionUri, model) {
  const css = webview.asWebviewUri(vscodeApi.Uri.joinPath(extensionUri, "media", "variables.css"));
  let content;
  if (model.kind === "ready") content = readyHtml(model);
  else if (model.kind === "error") content = `<section class="empty"><h1>Variables</h1><p class="notice error" role="alert">${escapeHtml(model.message)}</p></section>`;
  else content = `<section class="empty"><h1>Variables</h1><p>${escapeHtml(model.message)}</p></section>`;
  return `<!doctype html><html lang="es"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource}"><link rel="stylesheet" href="${css}"><title>PyxPic: Variables</title></head><body><main>${content}</main></body></html>`;
}

/** El mensaje cuando la CLI no pudo dar la lista: la CLI ausente o lo que dijo. */
function failureMessage(result) {
  if (result?.error && (result.error.code === "ENOENT" || /ENOENT/.test(String(result.error.message)))) {
    return "No se encontró la CLI de PyxPic: configura pyxpic.executable.";
  }
  const lines = `${result?.stderr || ""}\n${result?.stdout || ""}`.split(/\r?\n/).filter((line) => line.trim());
  return lines.length ? lines.slice(0, 2).join(" · ") : "La CLI no pudo leer las variables. Consulta la salida PyxPic.";
}

/**
 * La vista Variables. `source()` devuelve { key, file, text } del archivo BASIC activo (o `undefined`); `run(text)` ejecuta `pyxpic vars - --format json`
 * con el texto por la entrada estándar (así la lista sigue lo que se escribe, sin guardar) y devuelve { error, stdout, stderr }.
 */
function register(vscode, context, source, run) {
  let view;
  let latest;
  let timer;
  const render = async () => {
    if (!view) return;
    const request = Symbol();
    latest = request;
    const current = source();
    let model;
    if (!current) {
      model = { kind: "empty", message: "Abre un archivo PyxPic BASIC (.bas) para ver sus variables." };
    } else {
      const result = await run(current.text);
      if (latest !== request) return;
      const parsed = result?.error && !result.stdout ? { ok: false, error: failureMessage(result) } : parseVariablesReport(result?.stdout);
      model = parsed.ok ? { kind: "ready", report: parsed.report, file: current.file, stale: false } : { kind: "error", message: parsed.error === "La CLI no devolvió la lista de variables (JSON)." ? failureMessage(result) : parsed.error };
    }
    if (latest !== request) return;
    view.webview.html = html(view.webview, context.extensionUri, model);
  };
  const provider = {
    resolveWebviewView(webviewView) {
      view = webviewView;
      view.webview.options = { enableCommandUris: ["pyxpic.revealLine"], localResourceRoots: [vscode.Uri.joinPath(context.extensionUri, "media")] };
      void render();
    },
  };
  /** Mientras se escribe: espera una pausa corta y vuelve a leer la lista. */
  const schedule = () => {
    clearTimeout(timer);
    timer = setTimeout(() => { void render(); }, 350);
  };
  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider("pyxpic.variables", provider, { webviewOptions: { retainContextWhenHidden: true } }),
    vscode.window.onDidChangeActiveTextEditor(() => { void render(); }),
    { dispose: () => clearTimeout(timer) },
  );
  return { refresh: () => { void render(); }, schedule };
}

module.exports = { REPORT_VERSION, failureMessage, groupVariables, html, parseVariablesReport, register };
