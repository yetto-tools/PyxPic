"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync } = require("node:child_process");

const root = path.resolve(__dirname, "../..");
const binary = path.join(root, "target", "debug", process.platform === "win32" ? "pyxpic.exe" : "pyxpic");
const devices = path.join(root, "devices");
const snippets = JSON.parse(fs.readFileSync(path.join(__dirname, "../snippets/pyxpic-basic.json"), "utf8"));

/** El texto que el editor inserta con los valores por defecto de cada ${n:valor}. */
function expand(body) {
  return (Array.isArray(body) ? body.join("\n") : body).replace(/\$\{\d+:([^}]*)\}/g, "$1").replace(/\$\d+/g, "").replace(/\t/g, "  ");
}

test("el manifiesto contribuye las plantillas de PyxPic BASIC", () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, "../package.json"), "utf8"));
  assert.deepEqual(manifest.contributes.snippets, [{ language: "pyxpic-basic", path: "./snippets/pyxpic-basic.json" }]);
  assert.ok(manifest.files.includes("snippets"));
});

test("hay plantillas para todos los bucles de PICAXE y cada una tiene prefijo, descripcion y cuerpo", () => {
  const prefixes = Object.values(snippets).flatMap((s) => [].concat(s.prefix));
  for (const wanted of ["dowhile", "dountil", "doloopwhile", "doloopuntil", "do", "for", "forstep", "wend"]) assert.ok(prefixes.includes(wanted), wanted);
  for (const [name, snippet] of Object.entries(snippets)) {
    assert.ok(snippet.prefix && snippet.description && snippet.body, name);
  }
  assert.equal(expand(snippets["FOR ... NEXT"].body), "FOR b0 = 1 TO 10\n  \nNEXT b0");
  assert.equal(expand(snippets["DO WHILE ... LOOP"].body), "DO WHILE b0 < 10\n  \nLOOP");
});

test("cada plantilla, con sus valores por defecto, compila con la CLI real", () => {
  assert.ok(fs.existsSync(binary), "ejecuta cargo build -p pyxpic-cli antes de las pruebas");
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "pyxpic-snippets-"));
  for (const [name, snippet] of Object.entries(snippets)) {
    let body = expand(snippet.body);
    // una subrutina se llama con GOSUB y se salta para que el programa no entre en ella por error
    if (/RETURN\s*$/.test(body)) body = `GOSUB ${body.split(":")[0]}\nGOTO fin\n${body}\nfin:`;
    const program = `DEVICE PIC16F887\nCLOCK INTERNAL 4 MHZ\nBAUD 9600\n${body}\nEND\n`;
    const file = path.join(dir, "plantilla.bas");
    fs.writeFileSync(file, program);
    try {
      execFileSync(binary, ["check", file, "--devices-dir", devices], { stdio: "pipe", encoding: "utf8" });
    } catch (error) {
      assert.fail(`${name} no compila:\n${program}\n${error.stdout || ""}${error.stderr || ""}`);
    }
  }
});
