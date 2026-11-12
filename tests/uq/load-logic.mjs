// Loads the page's `.pragma library` scripts (uq/pages/scdr/*.js) into plain Node contexts.
// Each file gets its own context, as in QML; `.import "logic.js" as Logic` becomes a `Logic`
// global holding logic.js's context.
import { readFileSync } from "node:fs";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));
export const pageDir = `${here}../../uq/pages/scdr/`;
export const logicPath = `${pageDir}logic.js`;

function load(name, globals) {
  const path = `${pageDir}${name}`;
  const source = readFileSync(path, "utf8")
    .split("\n")
    .filter((line) => !/^\s*\.(pragma|import)\b/.test(line))
    .join("\n");
  const context = vm.createContext({ ...globals });
  vm.runInContext(source, context, { filename: path });
  return context;
}

export function loadLogic() {
  return load("logic.js", {});
}

// Another script of the page, with logic.js as its `Logic`.
export function loadScript(name, logic = loadLogic()) {
  return load(name, { Logic: logic });
}
