// Critère du CDC (YA-10) : .vsix < 5 Mo. Écrit la taille et la liste des fichiers ; échoue au-delà.
import { statSync } from "node:fs";
import { execFileSync } from "node:child_process";

const LIMIT = 5_000_000;
const path = process.argv[2] ?? "yusagent.vsix";
const size = statSync(path).size;
console.log(`${path} : ${size} octets (limite ${LIMIT})`);
try {
  console.log(execFileSync("unzip", ["-l", path], { encoding: "utf8" }));
} catch {
  // `unzip` absent : la taille suffit au contrôle.
}
if (size >= LIMIT) {
  console.error(`ÉCHEC : ${size} octets, au-delà de ${LIMIT}`);
  process.exit(1);
}
