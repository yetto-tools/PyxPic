"use strict";

function escapeHtml(value) {
  return String(value).replace(/[&<>"]/g, (char) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[char]);
}

function row(status) {
  const action = status.command ? `<button class="row-button" type="button" data-command="${status.command}">${escapeHtml(status.action)}</button>` : "";
  return `<li class="tool-row"><div><h3>${escapeHtml(status.name)}</h3><code>${escapeHtml(status.value)}</code><p>${escapeHtml(status.detail)}</p></div><div class="row-action"><span class="state ${status.kind}">${escapeHtml(status.state)}</span>${action}</div></li>`;
}

function html(webview, extensionUri, data) {
  const css = webview.asWebviewUri(require("vscode").Uri.joinPath(extensionUri, "media", "welcome.css"));
  const nonce = `${Date.now()}${Math.random()}`;
  return `<!doctype html>
<html lang="es"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource}; script-src 'nonce-${nonce}'"><link rel="stylesheet" href="${css}"><title>PyxPic IDE</title></head>
<body><main>
  <header class="masthead"><div><p class="brand">PyxPic IDE</p><h1>Empieza con tu firmware.</h1><p class="lead">Crea un proyecto, confirma las herramientas y compila código Python a firmware nativo para tu PIC.</p></div><button class="primary" data-command="newProject">Crear proyecto</button></header>
  <section class="start" aria-labelledby="start-title"><div><h2 id="start-title">Primeros pasos</h2><p>${escapeHtml(data.projectText)}</p></div><div class="actions"><button class="secondary" data-command="openProject">Abrir proyecto</button><button class="secondary" data-command="openFile">Abrir archivo</button><button class="secondary" data-command="openExample">Abrir ejemplo</button><button class="secondary" data-command="check">Comprobar archivo activo</button></div></section>
  <section aria-labelledby="environment-title"><div class="section-heading"><h2 id="environment-title">Entorno</h2><button class="quiet" data-command="refresh">Actualizar estado</button></div><p class="section-copy">Las rutas se guardan en el proyecto cuando hay una carpeta de trabajo abierta.</p><ul class="tool-list">${data.tools.map(row).join("")}</ul></section>
  <section class="limits" aria-labelledby="limits-title"><h2 id="limits-title">Disponible ahora</h2><div class="limits-grid"><div><h3>Compilar y comprobar</h3><p>La CLI genera Intel HEX y muestra diagnósticos en Problemas.</p></div><div><h3>Dispositivo</h3><p>${escapeHtml(data.device)}</p></div><div><h3>Próximamente</h3><p>Programación física, terminal serial, informe visual de memoria y depuración necesitan proveedores que todavía no existen.</p></div></div></section>
</main><script nonce="${nonce}">const vscode=acquireVsCodeApi();requestAnimationFrame(()=>requestAnimationFrame(()=>window.scrollTo({top:0,behavior:'instant'})));document.addEventListener('click',(event)=>{const control=event.target.closest('[data-command]');if(control)vscode.postMessage({command:control.dataset.command});});</script></body></html>`;
}

module.exports = { html };
