// Loads uq/pages/scdr/logic.js (a QML `.pragma library` file) into a plain Node context.
import { readFileSync } from "node:fs";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));
export const logicPath = `${here}../../uq/pages/scdr/logic.js`;

export function loadLogic() {
  const source = readFileSync(logicPath, "utf8")
    .split("\n")
    .filter((line) => !/^\s*\.(pragma|import)\b/.test(line))
    .join("\n");
  const context = vm.createContext({});
  vm.runInContext(source, context, { filename: logicPath });
  return context;
}
