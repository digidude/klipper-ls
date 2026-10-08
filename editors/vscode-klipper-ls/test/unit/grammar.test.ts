import assert from "node:assert/strict";
import fs from "node:fs";
import { createRequire } from "node:module";
import test from "node:test";

const require = createRequire(import.meta.url);
const vsctm = require("vscode-textmate");
const oniguruma = require("vscode-oniguruma");

const wasm = fs.readFileSync(require.resolve("vscode-oniguruma/release/onig.wasm")).buffer;
const onig = oniguruma.loadWASM(wasm).then(() => ({
  createOnigScanner: (p: string[]) => new oniguruma.OnigScanner(p),
  createOnigString: (s: string) => new oniguruma.OnigString(s),
}));
const registry = new vsctm.Registry({
  onigLib: onig,
  loadGrammar: async () =>
    vsctm.parseRawGrammar(fs.readFileSync("syntaxes/klipper.tmLanguage.json", "utf8"), "klipper.json"),
});

/** [text, scopes...] for every token in the document. */
async function tokens(source: string) {
  const grammar = await registry.loadGrammar("source.klipper");
  let stack = vsctm.INITIAL;
  const out: Record<string, string[]> = {};
  for (const line of source.split("\n")) {
    const r = grammar.tokenizeLine(line, stack);
    for (const t of r.tokens) out[line.slice(t.startIndex, t.endIndex)] = t.scopes;
    stack = r.ruleStack;
  }
  return out;
}

const has = (scopes: string[] | undefined, prefix: string) => !!scopes?.some((s) => s.startsWith(prefix));

test("sections, options, comments", async () => {
  const t = await tokens("# note\n[heater_bed]\nmax_temp: 120\n[gcode_macro FOO]\n");
  assert.ok(has(t["# note"], "comment.line"));
  assert.ok(has(t["heater_bed"], "entity.name.type.section"));
  assert.ok(has(t["max_temp"], "support.type.property-name"));
  assert.ok(has(t["120"], "constant.numeric"));
  assert.ok(has(t["FOO"], "entity.name.section"));
});

test("pins", async () => {
  const t = await tokens("[stepper_x]\nendstop_pin: ^!EBBCan:PB6\n");
  assert.ok(has(t["EBBCan:PB6"], "constant.other.pin"), JSON.stringify(t));
});

test("macro body: G-code and Jinja", async () => {
  const src = [
    "[gcode_macro PRINT_START]",
    "gcode:",
    "    {% set BED = params.BED|default(60)|float %}",
    "    M140 S{BED}",
    "    HEAT_SOAK TIME=5",
    "# col-0 comment does not end the value",
    "    M104 S0",
    "[next]",
    "a: 1",
  ].join("\n");
  const t = await tokens(src);
  assert.ok(has(t["set"], "keyword.control.jinja"));
  assert.ok(has(t["float"], "support.function.filter"));
  assert.ok(has(t["params"], "support.variable"));
  assert.ok(has(t["M140"], "entity.name.function.gcode"));
  assert.ok(has(t["HEAT_SOAK"], "entity.name.function.gcode"));
  assert.ok(has(t["TIME"], "variable.parameter"));
  assert.ok(has(t["M104"], "entity.name.function.gcode"), "value must continue past a col-0 comment");
  assert.ok(has(t["next"], "entity.name.type.section"), "next section must end the macro");
  assert.ok(has(t["a"], "support.type.property-name"));
});
