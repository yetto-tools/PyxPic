"use strict";

const crypto = require("node:crypto");
const vscodeApi = require("vscode");

/** La versión del formato de `pyxpic monitor --format jsonl` y `--list --format json` que esta vista entiende (docs/monitor-serial.md). */
const FORMAT_VERSION = 1;
const MAX_LINES = 2000;

function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (char) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[char]);
}

/** Los puertos de `pyxpic monitor --list --format json`. */
function parsePorts(text) {
  try {
    const report = JSON.parse(String(text));
    if (report.version !== FORMAT_VERSION) return { ok: false, error: `La versión ${report.version} de la lista de puertos no la entiende esta extensión (entiende la ${FORMAT_VERSION}): actualiza la extensión o la CLI.` };
    return { ok: true, ports: (report.ports || []).map((p) => ({ name: String(p.name), label: p.product ? `${p.name} · ${p.product}` : String(p.name) })) };
  } catch {
    return { ok: false, error: "La CLI no devolvió la lista de puertos (JSON)." };
  }
}

/** Una línea de `--format jsonl` como algo que mostrar: { kind: "data"|"info"|"error", text, t_ms }. Lo que no se entiende se muestra tal cual. */
function parseEvent(line) {
  let event;
  try {
    event = JSON.parse(line);
  } catch {
    return { kind: "info", text: line };
  }
  if (event.version !== FORMAT_VERSION) return { kind: "error", text: `Evento con la versión ${event.version}, que esta extensión no entiende: actualiza la extensión o la CLI.` };
  if (event.type === "data") return { kind: "data", text: String(event.text ?? ""), hex: String(event.hex ?? ""), t_ms: event.t_ms };
  if (event.type === "closed") return { kind: "error", text: String(event.message ?? "") };
  return { kind: "info", text: String(event.message ?? "") };
}

/** Los argumentos de `pyxpic monitor` para una sesión. `opts`: { port, sim, baud, filter, seconds, eol, sendHex }. */
function buildArgs(opts) {
  const args = ["monitor"];
  if (opts.sim) args.push("--sim", opts.sim, "--seconds", String(opts.seconds || 5));
  else args.push("--port", opts.port, "--baud", String(opts.baud || 9600));
  args.push("--format", "jsonl");
  if (opts.filter) args.push("--filter", opts.filter);
  if (opts.eol) args.push("--eol", opts.eol);
  if (opts.sendHex) args.push("--send-encoding", "hex");
  return args;
}

/** Parte la salida de un proceso en líneas completas (un tramo puede traer media línea). */
function lineSplitter(onLine) {
  let rest = "";
  return (chunk) => {
    rest += String(chunk);
    const parts = rest.split(/\r?\n/);
    rest = parts.pop();
    parts.filter(Boolean).forEach(onLine);
  };
}

function formatLine(item, options) {
  const t = item.t_ms;
  const stamp = options.timestamps && Number.isFinite(t) ? `[${String(Math.floor(t / 60000)).padStart(2, "0")}:${String(Math.floor(t / 1000) % 60).padStart(2, "0")}.${String(t % 1000).padStart(3, "0")}] ` : "";
  const body = options.encoding === "hex" && item.kind === "data" ? item.hex : item.text.replace(/(\\x0D)?\\x0A$/, "");
  return stamp + body;
}

function html(webview, extensionUri, model) {
  const css = webview.asWebviewUri(vscodeApi.Uri.joinPath(extensionUri, "media", "serial.css"));
  const nonce = crypto.randomBytes(16).toString("base64");
  const ports = model.ports.map((p) => `<option value="${escapeHtml(p.name)}"${p.name === model.port ? " selected" : ""}>${escapeHtml(p.label)}</option>`).join("");
  const bauds = [300, 1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200].map((b) => `<option${b === model.baud ? " selected" : ""}>${b}</option>`).join("");
  const error = model.portsError ? `<p class="notice error" role="alert">${escapeHtml(model.portsError)}</p>` : "";
  const script = `const vscode=acquireVsCodeApi();const log=document.querySelector('#log');const status=document.querySelector('#status');let lines=[];
const val=(id)=>document.querySelector('#'+id);
function opts(){return{port:val('port').value,baud:Number(val('baud').value),encoding:val('encoding').value,timestamps:val('timestamps').checked,filter:val('filter').value,eol:val('eol').value,sendHex:val('send-hex').checked};}
document.addEventListener('click',(e)=>{const b=e.target.closest('[data-command]');if(!b)return;const c=b.dataset.command;if(c==='clear'){lines=[];log.textContent='';return;}if(c==='copy'){vscode.postMessage({command:'copy',text:lines.join('\\n')});return;}vscode.postMessage({command:c,options:opts()});});
document.querySelector('#send-form').addEventListener('submit',(e)=>{e.preventDefault();const t=val('send');vscode.postMessage({command:'send',text:t.value});t.value='';});
window.addEventListener('message',(e)=>{const m=e.data;if(m.type==='line'){lines.push(m.text);if(lines.length>${MAX_LINES}){lines.shift();}const stick=log.scrollTop+log.clientHeight>=log.scrollHeight-8;const row=document.createElement('div');row.className=m.kind;row.textContent=m.text;log.appendChild(row);while(log.childElementCount>${MAX_LINES})log.firstChild.remove();if(stick)log.scrollTop=log.scrollHeight;}if(m.type==='status'){status.textContent=m.text;val('start').hidden=m.running;val('sim').hidden=m.running;val('stop').hidden=!m.running;}});`;
  return `<!doctype html><html lang="es"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource}; script-src 'nonce-${nonce}'"><link rel="stylesheet" href="${css}"><title>PyxPic: Terminal serial</title></head><body><main>
<header><h1>Terminal serial</h1><p id="status" role="status">Detenida</p></header>${error}
<section class="controls">
<label>Puerto<select id="port">${ports || "<option value=''>(sin puertos)</option>"}</select></label>
<label>Velocidad<select id="baud">${bauds}</select></label>
<label>Formato<select id="encoding"><option value="ascii">ASCII</option><option value="hex">Hexadecimal</option></select></label>
<label>Filtro<input id="filter" type="text" placeholder="expresión regular"></label>
<label>Fin de línea al enviar<select id="eol"><option value="lf">LF</option><option value="crlf">CR+LF</option><option value="cr">CR</option><option value="none">Nada</option></select></label>
<label class="check"><input id="timestamps" type="checkbox"> Marcas de tiempo</label>
<label class="check"><input id="send-hex" type="checkbox"> Enviar en hexadecimal (48 65 6C)</label>
<div class="buttons"><button id="start" data-command="start">Iniciar</button><button id="stop" data-command="stop" hidden>Detener</button><button id="sim" data-command="sim" title="Ejecuta el archivo activo en el simulador">Simular archivo</button><button data-command="refresh">Actualizar puertos</button></div>
<p class="hint">El fin de línea, el modo de envío y el filtro se aplican al iniciar.</p>
</section>
<div id="log" class="log" role="log" aria-live="off" tabindex="0"></div>
<div class="buttons"><button data-command="clear">Limpiar</button><button data-command="copy">Copiar</button></div>
<form id="send-form" class="send"><input id="send" type="text" aria-label="Datos a enviar" placeholder="Enviar…"><button type="submit">Enviar</button></form>
<p class="footnote">La terminal usa <code>pyxpic monitor</code>. El puerto lo abre la CLI: deténla antes de grabar el PIC.</p>
</main><script nonce="${nonce}">${script}</script></body></html>`;
}

/**
 * La vista Terminal serial. `deps`: { listPorts() -> {error, stdout, stderr}, spawn(args) -> proceso con stdout/stderr/stdin y eventos "close"/"error",
 * activeSource() -> ruta del archivo activo o undefined }. Un proceso por sesión: detener lo termina y un error se muestra en el registro.
 */
function register(vscode, context, deps) {
  let view;
  let child;
  const model = { ports: [], port: undefined, baud: 9600, portsError: undefined };
  const post = (message) => { void view?.webview.postMessage(message); };
  const say = (kind, text) => post({ type: "line", kind, text });

  async function refreshPorts() {
    const result = await deps.listPorts();
    const parsed = result?.error && !result.stdout ? { ok: false, error: /ENOENT/.test(String(result.error.message)) ? "No se encontró la CLI de PyxPic: configura pyxpic.executable (PyxPic: Seleccionar compilador)." : "La CLI no pudo listar los puertos." } : parsePorts(result?.stdout);
    model.ports = parsed.ok ? parsed.ports : [];
    model.portsError = parsed.ok ? undefined : parsed.error;
    if (view) view.webview.html = html(view.webview, context.extensionUri, model);
  }

  function stop() {
    if (!child) return;
    const proc = child;
    child = undefined;
    try { proc.kill(); } catch { /* ya terminó */ }
    post({ type: "status", text: "Detenida", running: false });
  }

  function start(opts, sim) {
    stop();
    let args;
    if (sim) {
      const file = deps.activeSource();
      if (!file) { say("error", "Abre un archivo PyxPic (.py, .bas) para simularlo."); return; }
      args = buildArgs({ ...opts, sim: file });
    } else {
      if (!opts.port) { say("error", "Elige un puerto (conecta el adaptador y pulsa Actualizar puertos)."); return; }
      args = buildArgs(opts);
    }
    model.port = opts.port;
    model.baud = opts.baud;
    let proc;
    try {
      proc = deps.spawn(args);
    } catch (error) {
      say("error", `No se pudo iniciar la CLI: ${error.message}`);
      return;
    }
    child = proc;
    post({ type: "status", text: sim ? "Simulando…" : `Conectada a ${opts.port}`, running: true });
    proc.stdout.on("data", lineSplitter((line) => {
      const item = parseEvent(line);
      say(item.kind, formatLine(item, opts));
    }));
    proc.stderr.on("data", lineSplitter((line) => say("error", line)));
    proc.on("error", (error) => {
      say("error", error.code === "ENOENT" ? "No se encontró la CLI de PyxPic: configura pyxpic.executable." : `Error de la CLI: ${error.message}`);
      if (child === proc) { child = undefined; post({ type: "status", text: "Detenida", running: false }); }
    });
    proc.on("close", (code) => {
      if (child !== proc) return;
      child = undefined;
      say(code === 0 ? "info" : "error", code === 0 ? "Sesión terminada." : `La sesión terminó con el código ${code}.`);
      post({ type: "status", text: "Detenida", running: false });
    });
  }

  function send(message) {
    if (!child?.stdin) { say("error", "No hay una sesión abierta: pulsa Iniciar."); return; }
    child.stdin.write(`${message.text}\n`);
  }

  const provider = {
    resolveWebviewView(webviewView) {
      view = webviewView;
      view.webview.options = { enableScripts: true, localResourceRoots: [vscode.Uri.joinPath(context.extensionUri, "media")] };
      view.webview.onDidReceiveMessage((message) => {
        if (message.command === "start") start(message.options, false);
        else if (message.command === "sim") start(message.options, true);
        else if (message.command === "stop") stop();
        else if (message.command === "refresh") void refreshPorts();
        else if (message.command === "send") send(message);
        else if (message.command === "copy") void vscode.env.clipboard.writeText(String(message.text || ""));
      });
      view.onDidDispose(() => { stop(); view = undefined; });
      void refreshPorts();
    },
  };
  context.subscriptions.push(vscode.window.registerWebviewViewProvider("pyxpic.serial", provider, { webviewOptions: { retainContextWhenHidden: true } }), { dispose: stop });
  return { refresh: refreshPorts, stop, isRunning: () => Boolean(child) };
}

module.exports = { FORMAT_VERSION, buildArgs, formatLine, html, lineSplitter, parseEvent, parsePorts, register };
