"use strict";

const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const vscodeApi = require("vscode");

function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (char) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[char]);
}

function readDevice(devicesDir, deviceName) {
  if (!devicesDir) return { kind: "error", message: "Configura la carpeta de Device Packs para mostrar el pinout." };
  const deviceDir = path.join(devicesDir, deviceName.toLowerCase());
  try {
    const device = JSON.parse(fs.readFileSync(path.join(deviceDir, "device.json"), "utf8"));
    const pins = JSON.parse(fs.readFileSync(path.join(deviceDir, "pins.json"), "utf8"));
    const packages = Array.isArray(pins.packages) ? pins.packages : [];
    const packagesValid = packages.length > 0 && packages.every((packageInfo) =>
      typeof packageInfo.name === "string" &&
      Number.isInteger(packageInfo.pin_count) &&
      Array.isArray(packageInfo.pins) &&
      packageInfo.pins.length === packageInfo.pin_count,
    );
    if (device?.name !== deviceName || !packagesValid) {
      return { kind: "error", message: `El Device Pack de ${deviceName} no contiene encapsulados válidos.` };
    }
    return { kind: "ready", device, packages };
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    return { kind: "error", message: `No se pudo cargar el Device Pack de ${deviceName}: ${detail}` };
  }
}

function catalogDevices(devicesDir) {
  if (!devicesDir) return [];
  try {
    return fs.readdirSync(devicesDir, { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .flatMap((entry) => {
        try {
          const metadata = JSON.parse(fs.readFileSync(path.join(devicesDir, entry.name, "device.json"), "utf8"));
          if (typeof metadata?.name !== "string") return [];
          const model = readDevice(devicesDir, metadata.name);
          return model.kind === "ready" ? [{ name: model.device.name, family: model.device.family, model }] : [];
        } catch (error) {
          if (error instanceof Error) return [];
          throw error;
        }
      })
      .sort((a, b) => a.family.localeCompare(b.family) || a.name.localeCompare(b.name));
  } catch (error) {
    if (error instanceof Error) return [];
    throw error;
  }
}

function resolveModel({ catalog, detected, selectedName, selectedPackage, notice }) {
  if (catalog.length === 0) return { kind: "error", message: "No hay Device Packs con pinout en la carpeta configurada." };
  const selectedPack = typeof selectedName === "string" && catalog.find((entry) => entry.name.toLowerCase() === selectedName.toLowerCase());
  const detectedPack = detected && catalog.find((entry) => entry.name.toLowerCase() === detected.name.toLowerCase());
  const selectPackage = (model) => model.packages.find((entry) => entry.name === selectedPackage) || model.packages[0];
  if (selectedPack) return { ...selectedPack.model, packages: [selectPackage(selectedPack.model), ...selectedPack.model.packages.filter((entry) => entry.name !== selectPackage(selectedPack.model).name)], catalog, detectedName: detected?.name, selectionMode: "manual", source: "Selección de visualización" };
  if (detectedPack) return { ...detectedPack.model, packages: [selectPackage(detectedPack.model), ...detectedPack.model.packages.filter((entry) => entry.name !== selectPackage(detectedPack.model).name)], catalog, detectedName: detected.name, selectionMode: "automatic", source: detected.source };
  const selected = catalog[0];
  const reason = detected
    ? `No hay un Device Pack instalado para ${detected.name}. Se muestra ${selected.name}.`
    : notice || "No se detectó un PIC en el archivo o perfil activo.";
  return { ...selected.model, packages: [selectPackage(selected.model), ...selected.model.packages.filter((entry) => entry.name !== selectPackage(selected.model).name)], catalog, detectedName: detected?.name, selectionMode: "automatic", source: "Selección de visualización", notice: reason };
}

function detectedDevice(declaredName, report, selectedTarget) {
  if (declaredName) return { name: declaredName, source: "Detectado en el archivo activo" };
  if (!report) return undefined;
  const targetName = selectedTarget || report.defaultTarget || (report.targets.length === 1 ? report.targets[0].name : undefined);
  const target = report.targets.find((entry) => entry.name === targetName);
  return target?.device ? { name: target.device, source: `Perfil de proyecto: ${target.name}` } : undefined;
}

function pinLabel(pin) {
  return [pin.name, ...(Array.isArray(pin.functions) ? pin.functions : [])].join(" / ");
}

const KIND_LABELS = { digital: "Digital", analog: "Digital y analógico", power: "Alimentación", input: "Solo entrada" };

function pinKind(pin) {
  const functions = Array.isArray(pin.functions) ? pin.functions : [];
  return pin.power ? "power" : pin.input_only ? "input" : functions.some((name) => name.startsWith("AN")) ? "analog" : "digital";
}

const DETAIL_ROWS = [["registers", "Registros"], ["current", "Corriente"], ["levels", "Niveles"], ["pullup", "Pull-up"], ["snippet", "En PyxPic"]];
const num = (value) => String(value).replace(".", ",");

/** Lo que el programa necesita saber de un pin: registros, limites electricos (del Device Pack), pull-up y como se importa en PyxPic. */
function pinInfo(pin, electrical) {
  const functions = Array.isArray(pin.functions) ? pin.functions : [];
  const gpio = !pin.power && pin.port && Number.isInteger(pin.bit);
  const channel = functions.find((name) => /^AN\d+$/.test(name));
  const registers = gpio ? [`PORT${pin.port}<${pin.bit}>`, `TRIS${pin.port}<${pin.bit}>`, pin.weak_pullup ? registerBit(pin.weak_pullup) : undefined, pin.interrupt_on_change ? `${registerBit(pin.interrupt_on_change)} (interrupción por cambio)` : undefined, channel ? `canal analógico ${channel}` : undefined].filter(Boolean).join(" · ") : "";
  const snippet = gpio && /^R[A-Z]\d$/.test(pin.name) ? `from pyxpic.periph.gpio import ${pin.name}` : "";
  const info = { registers, current: "", levels: "", pullup: "", snippet };
  if (!electrical) return info;
  const { io, supply, weak_pullup: pullup } = electrical;
  if (pin.power) {
    info.current = pin.name === "VSS" ? `Máx. ${num(supply.max_vss_out_ma)} mA saliendo por VSS (suma de todos los pines)` : `Máx. ${num(supply.max_vdd_in_ma)} mA entrando por VDD (suma de todos los pines)`;
  } else if (pin.input_only) {
    info.current = "Solo entrada: sin salida que excitar";
  } else {
    info.current = `Máx. absoluto ${num(io.max_sink_ma)} mA por pin (sumidero) y ${num(io.max_source_ma)} mA (fuente); es un límite de daño, no de uso continuo`;
    info.levels = `Salida baja ≤ ${num(io.vol_v)} V a ${num(io.vol_at_ma)} mA · alta ≥ VDD − ${num(io.voh_below_vdd_v)} V a ${num(io.voh_at_ma)} mA (VDD = ${num(io.test_vdd_v)} V) · fuga de entrada ±${num(io.leakage_ua)} µA`;
    info.pullup = pin.weak_pullup && pullup
      ? `Sí: ${registerBit(pin.weak_pullup)} = 1 con ${pullup.global_disable} = 0 · ${num(pullup.min_ua)} a ${num(pullup.max_ua)} µA (típ. ${num(pullup.typ_ua)} µA a 5 V) · se apaga si el pin es salida${channel ? " o analógico" : ""}`
      : "Sin pull-up interno: usa una resistencia externa";
  }
  return info;
}

/** `WPUB3` -> `WPUB<3>` */
function registerBit(name) {
  const match = /^(.*?)(\d+)$/.exec(name);
  return match ? `${match[1]}<${match[2]}>` : name;
}

function pinButton(pin, side, selected = false, electrical) {
  const functions = Array.isArray(pin.functions) ? pin.functions : [];
  const kind = pinKind(pin);
  const kindLabel = KIND_LABELS[kind];
  const tooltip = [
    `Pin ${pin.number}`,
    pin.port ? `PORT${pin.port}` : undefined,
    pin.name,
    `Tipo: ${kindLabel}`,
    functions.length ? `Funciones: ${functions.join(", ")}` : "Funciones: GPIO digital",
  ].filter(Boolean).join(" · ");
  const attributes = [
    `data-number="${escapeHtml(pin.number)}"`,
    `data-name="${escapeHtml(pin.name)}"`,
    `data-functions="${escapeHtml(functions.join(", ") || "GPIO digital")}"`,
    `data-kind="${kind}"`,
    ...DETAIL_ROWS.map(([key]) => `data-${key}="${escapeHtml(pinInfo(pin, electrical)[key])}"`),
    `title="${escapeHtml(tooltip)}"`,
    `aria-label="Pin ${escapeHtml(pin.number)}: ${escapeHtml(pin.port ? `PORT${pin.port}, ${pinLabel(pin)}` : pinLabel(pin))}"`,
  ].join(" ");
  return `<button class="pin pin-${side} ${kind}${selected ? " selected" : ""}" type="button" aria-pressed="${selected}" ${attributes}><span class="pin-copy"><strong>${escapeHtml(pin.name)}</strong><small>${escapeHtml(functions.join(" · ") || (pin.power ? "Alimentación" : "GPIO"))}</small></span><span class="pin-number">${escapeHtml(pin.number)}</span></button>`;
}

function packageDiagram(packageInfo, deviceName, selectedPin) {
  const midpoint = packageInfo.pin_count / 2;
  const left = packageInfo.pins.filter((pin) => pin.number <= midpoint).sort((a, b) => a.number - b.number);
  const right = packageInfo.pins.filter((pin) => pin.number > midpoint).sort((a, b) => b.number - a.number);
  const groups = (pins) => {
    const result = [];
    for (const pin of pins) {
      const port = pin.port ? `PORT${pin.port}` : "";
      const last = result.at(-1);
      if (last && last.port === port) last.pins.push(pin);
      else result.push({ port, pins: [pin] });
    }
    return result;
  };
  const bank = (pins, side) => groups(pins).map((group) => `<div class="port-group ${side} ${group.port ? "has-port" : "no-port"}" style="--pin-count:${group.pins.length}"${group.port ? ` aria-label="${escapeHtml(group.port)}"` : ""}>${group.port ? `<span class="port-brace" aria-hidden="true"></span><span class="port-label">${escapeHtml(group.pins.length > 1 ? group.port : group.port.slice(-1))}</span>` : ""}${group.pins.map((pin) => pinButton(pin, side, pin === selectedPin, packageInfo.electrical)).join("")}</div>`).join("");
  return `<div class="package" aria-label="Diagrama ${escapeHtml(packageInfo.name)}">
    <div class="pin-bank">${bank(left, "left")}</div>
    <div class="chip"><span class="notch" aria-hidden="true"></span><strong>${escapeHtml(deviceName)}</strong><span>${escapeHtml(packageInfo.name)}</span></div>
    <div class="pin-bank">${bank(right, "right")}</div>
  </div>`;
}

function detailRow(key, label, text) {
  return `<p class="detail-row" id="detail-${key}-row"${text ? "" : " hidden"}><span>${label}</span>${key === "snippet" ? `<code id="detail-${key}">${escapeHtml(text)}</code>` : `<b id="detail-${key}">${escapeHtml(text)}</b>`}</p>`;
}

function readyHtml(model) {
  const packageInfo = model.packages[0];
  const first = packageInfo.pins[0];
  const automaticLabel = model.detectedName ? `Automático · ${model.detectedName}` : "Automático · sin detectar";
  const packageSelector = `<label class="package-picker" for="package-picker"><span>Encapsulado</span><select id="package-picker" data-command="selectPackage">${model.packages.map((entry) => `<option value="${escapeHtml(entry.name)}"${entry.name === packageInfo.name ? " selected" : ""}>${escapeHtml(entry.name)} · ${escapeHtml(entry.pin_count)} pines</option>`).join("")}</select></label>`;
  const selector = `<label class="device-picker" for="device-picker"><span>Visualizar PIC</span><select id="device-picker" data-command="selectDevice"><option value=""${model.selectionMode === "automatic" ? " selected" : ""}>${escapeHtml(automaticLabel)}</option>${model.catalog.map((entry) => `<option value="${escapeHtml(entry.name)}"${model.selectionMode === "manual" && entry.name === model.device.name ? " selected" : ""}>${escapeHtml(entry.name)} · ${escapeHtml(entry.family)}</option>`).join("")}</select><small>Solo cambia esta vista; no modifica el proyecto.</small></label>`;
  const notice = model.notice ? `<p class="notice">${escapeHtml(model.notice)}</p>` : "";
  return `<header class="pinout-head"><h1>${escapeHtml(model.device.name)}</h1><p>${escapeHtml(model.source)}</p><small>${escapeHtml(model.device.family)} · ${escapeHtml(packageInfo.name)} · ${escapeHtml(packageInfo.pin_count)} pines</small></header>
  ${notice}${selector}${packageSelector}
  <section class="legend" aria-label="Tipos de pin"><span><i class="digital"></i>Digital</span><span><i class="analog"></i>Analógico</span><span><i class="power"></i>Alimentación</span><span><i class="input"></i>Solo entrada</span></section>
  <section class="diagram-wrap">${packageDiagram(packageInfo, model.device.name, first)}</section>
  <section class="pin-detail" aria-live="polite"><div class="detail-title"><span>Pin <strong id="detail-number">${escapeHtml(first.number)}</strong></span><h2 id="detail-name">${escapeHtml(first.name)}</h2></div><p class="detail-kind" id="detail-kind" data-kind="${pinKind(first)}">${KIND_LABELS[pinKind(first)]}</p><p class="detail-row" id="detail-functions-row"><span>Funciones</span><b id="detail-functions">${escapeHtml(first.functions.join(", ") || "GPIO digital")}</b></p>${DETAIL_ROWS.map(([key, label]) => detailRow(key, label, pinInfo(first, packageInfo.electrical)[key])).join("")}</section>
  <p class="source">Fuente del Device Pack: ${escapeHtml(packageInfo.source)}</p>`;
}

function html(webview, extensionUri, model) {
  const css = webview.asWebviewUri(vscodeApi.Uri.joinPath(extensionUri, "media", "pinout.css"));
  const nonce = crypto.randomBytes(16).toString("base64");
  const content = model.kind === "ready" ? readyHtml(model) : `<section class="empty"><h1>Pinout no disponible</h1><p>${escapeHtml(model.message)}</p><button type="button" data-command="selectDevices">Configurar Device Packs</button></section>`;
  return `<!doctype html><html lang="es"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource}; script-src 'nonce-${nonce}'"><link rel="stylesheet" href="${css}"><title>PyxPic: Pinout</title></head><body><main>${content}</main><script nonce="${nonce}">const vscode=acquireVsCodeApi();const labels={digital:'Digital',analog:'Digital y analógico',power:'Alimentación',input:'Solo entrada'};document.addEventListener('change',(event)=>{const picker=event.target.closest('select[data-command]');if(!picker)return;vscode.postMessage({command:picker.dataset.command,device:picker.dataset.command==='selectDevice'?picker.value:undefined,packageName:picker.dataset.command==='selectPackage'?picker.value:undefined});});document.addEventListener('click',(event)=>{const command=event.target.closest('[data-command]');if(command&&command.tagName!=='SELECT'){vscode.postMessage({command:command.dataset.command});return;}const pin=event.target.closest('.pin');if(!pin)return;const old=document.querySelector('.pin.selected');if(old){old.classList.remove('selected');old.setAttribute('aria-pressed','false');}pin.classList.add('selected');pin.setAttribute('aria-pressed','true');document.querySelector('#detail-number').textContent=pin.dataset.number;document.querySelector('#detail-name').textContent=pin.dataset.name;document.querySelector('#detail-functions').textContent=pin.dataset.functions;for(const key of ['registers','current','levels','pullup','snippet']){document.querySelector('#detail-'+key).textContent=pin.dataset[key];document.querySelector('#detail-'+key+'-row').hidden=!pin.dataset[key];}const kind=document.querySelector('#detail-kind');kind.textContent=labels[pin.dataset.kind];kind.dataset.kind=pin.dataset.kind;const calm=matchMedia('(prefers-reduced-motion: reduce)').matches;document.querySelectorAll('.pin-detail > *').forEach((part,index)=>part.animate(calm?[{opacity:.4},{opacity:1}]:[{opacity:0,transform:'translateY(3px)'},{opacity:1,transform:'none'}],{duration:calm?120:170,delay:calm?0:index*25,easing:'cubic-bezier(0.16, 1, 0.3, 1)',fill:'backwards'}));});</script></body></html>`;
}

function register(vscode, context, currentDevice) {
  let view;
  let latestRequest;
  let visibleCatalog = [];
  const render = async () => {
    if (!view) return;
    const request = Symbol();
    latestRequest = request;
    const current = await currentDevice();
    if (latestRequest !== request) return;
    visibleCatalog = catalogDevices(current.devicesDir);
    const selected = context.workspaceState.get("pyxpic.pinout.deviceOverride");
    const selectedPackage = context.workspaceState.get("pyxpic.pinout.packageOverride");
    view.webview.html = html(view.webview, context.extensionUri, resolveModel({ catalog: visibleCatalog, detected: current.detected, selectedName: selected, selectedPackage, notice: current.notice }));
  };
  const provider = {
    resolveWebviewView(webviewView) {
      view = webviewView;
      view.webview.options = { enableScripts: true, localResourceRoots: [vscode.Uri.joinPath(context.extensionUri, "media")] };
      view.webview.onDidReceiveMessage(async (message) => {
        if (message?.command === "selectDevices") await vscode.commands.executeCommand("pyxpic.selectDevices");
        if (message?.command === "selectDevice" && typeof message.device === "string" && (message.device === "" || visibleCatalog.some((entry) => entry.name === message.device))) {
          await context.workspaceState.update("pyxpic.pinout.deviceOverride", message.device || undefined);
        }
        if (message?.command === "selectPackage" && typeof message.packageName === "string") {
          await context.workspaceState.update("pyxpic.pinout.packageOverride", message.packageName || undefined);
        }
        await render();
      }, undefined, context.subscriptions);
      void render();
    },
  };
  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider("pyxpic.pinout", provider, { webviewOptions: { retainContextWhenHidden: true } }),
    vscode.commands.registerCommand("pyxpic.pinout", () => vscode.commands.executeCommand("pyxpic.pinout.focus")),
    vscode.window.onDidChangeActiveTextEditor(() => { void render(); }),
  );
  return { refresh: () => { void render(); } };
}

module.exports = { catalogDevices, detectedDevice, html, readDevice, register, resolveModel };
