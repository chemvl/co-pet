// Signs the bundled llama.cpp binaries in src-tauri/resources/llama/ with the Developer ID
// (hardened runtime + secure timestamp), dylibs first and llama-server last, then verifies each.
import { existsSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";

const identity = process.env.APPLE_SIGNING_IDENTITY;
if (!identity) {
  console.error("APPLE_SIGNING_IDENTITY is not set");
  process.exit(1);
}

const dir = join(resolve(import.meta.dir, ".."), "src-tauri", "resources", "llama");
if (!existsSync(join(dir, ".version"))) {
  console.error("run bun run fetch:llama first");
  process.exit(1);
}

const names = readdirSync(dir);
const files = [
  ...names.filter((n) => n.endsWith(".dylib")).sort(),
  ...names.filter((n) => n === "llama-server"),
].map((n) => join(dir, n));

function run(args: string[]): boolean {
  return Bun.spawnSync(["codesign", ...args], { stdout: "inherit", stderr: "inherit" }).exitCode === 0;
}

for (const file of files) {
  if (!run(["--force", "--options", "runtime", "--timestamp", "-s", identity, file])) {
    console.error(`codesign failed: ${file}`);
    process.exit(1);
  }
}
for (const file of files) {
  if (!run(["--verify", "--strict", file])) {
    console.error(`codesign --verify failed: ${file}`);
    process.exit(1);
  }
}
console.log(`signed ${files.length} files in ${dir}`);
