const test = require("node:test");
const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");
const { registerSpelling, SPELL_CHECKER } = require("../spelling");

const root = path.join(__dirname, "..");
const readJson = (file) => JSON.parse(fs.readFileSync(path.join(root, file), "utf8"));

function fakeVscode(extension) {
  return { extensions: { getExtension: (id) => (id === SPELL_CHECKER ? extension : undefined) } };
}

test("registra la configuración cuando está el corrector", async () => {
  const registered = [];
  const checker = { activate: async () => ({ registerConfig: (file) => registered.push(file) }) };
  assert.strictEqual(await registerSpelling(fakeVscode(checker), root), true);
  assert.deepStrictEqual(registered, [path.join(root, "cspell", "cspell-ext.json")]);
  assert.ok(fs.existsSync(registered[0]));
});

test("sin el corrector, sin su API o si falla, no rompe la extensión", async () => {
  assert.strictEqual(await registerSpelling(fakeVscode(undefined), root), false);
  assert.strictEqual(await registerSpelling({}, root), false);
  assert.strictEqual(await registerSpelling(fakeVscode({ activate: async () => ({}) }), root), false);
  const messages = [];
  const broken = { activate: async () => { throw new Error("roto"); } };
  assert.strictEqual(await registerSpelling(fakeVscode(broken), root, (m) => messages.push(m)), false);
  assert.match(messages[0], /roto/);
});

test("la configuración cubre el modo pyxpic y se empaqueta", () => {
  const config = readJson("cspell/cspell-ext.json");
  const [settings] = config.languageSettings;
  assert.strictEqual(settings.languageId, "pyxpic");
  assert.deepStrictEqual(settings.dictionaries, ["python", "python-common", "pyxpic"]);
  for (const definition of config.dictionaryDefinitions) assert.ok(fs.existsSync(path.join(root, "cspell", definition.path)));
  const manifest = readJson("package.json");
  for (const file of ["spelling.js", "cspell"]) assert.ok(manifest.files.includes(file), file);
});
