import { copyFileSync, mkdirSync } from "node:fs";
const output = new URL("../public/methodology/", import.meta.url);
mkdirSync(output, { recursive: true });
for (const file of ["methodology.md", "receipts.md", "dataset.md", "limitations.md"]) {
  copyFileSync(new URL(`../../docs/${file}`, import.meta.url), new URL(file, output));
}
