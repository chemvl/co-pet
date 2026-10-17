// `bun run set-version <x.y.z>`: writes one version to tauri.conf.json, Cargo.toml and package.json.
// `readVersions()` is used by `scripts/release.ts` to check the three agree.
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");
const TAURI_CONF = join(root, "src-tauri/tauri.conf.json");
const CARGO_TOML = join(root, "src-tauri/Cargo.toml");
const PACKAGE_JSON = join(root, "package.json");

const VERSION_RE = /^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$/;
const CARGO_VERSION_RE = /^version = "([^"]*)"/m;

// Rewrites only the top-level `"version"` line (2-space indent) so the rest of the file keeps its
// formatting; re-parses the result to confirm the JSON value changed.
function writeJsonVersion(path: string, version: string) {
  const text = readFileSync(path, "utf8");
  const next = text.replace(/^ {2}"version": "[^"]*"/m, `  "version": "${version}"`);
  if (JSON.parse(next).version !== version) throw new Error(`${path}: no top-level "version" line`);
  writeFileSync(path, next);
}

export function readVersions(): { tauri: string; cargo: string; npm: string } {
  const cargo = readFileSync(CARGO_TOML, "utf8").match(CARGO_VERSION_RE);
  if (!cargo) throw new Error("src-tauri/Cargo.toml has no version line");
  const tauri = JSON.parse(readFileSync(TAURI_CONF, "utf8")).version;
  const npm = JSON.parse(readFileSync(PACKAGE_JSON, "utf8")).version;
  return { tauri, cargo: cargo[1], npm };
}

function setVersion(version: string) {
  writeJsonVersion(TAURI_CONF, version);
  const toml = readFileSync(CARGO_TOML, "utf8");
  if (!CARGO_VERSION_RE.test(toml)) throw new Error("src-tauri/Cargo.toml has no version line");
  writeFileSync(CARGO_TOML, toml.replace(CARGO_VERSION_RE, `version = "${version}"`));
  writeJsonVersion(PACKAGE_JSON, version);
}

if (import.meta.main) {
  const version = process.argv[2];
  if (!version || !VERSION_RE.test(version)) {
    console.error("usage: bun run set-version <x.y.z> (optionally -prerelease)");
    process.exit(1);
  }
  setVersion(version);
  console.log(`version ${version}:`, readVersions());
}
