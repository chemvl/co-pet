// `bun run set-version <x.y.z>`: writes one version to tauri.conf.json, Cargo.toml, package.json and
// the `copet` package entry of Cargo.lock. Numeric only, each part up to 65535: the MSIX package
// version is `<x.y.z>.0` (scripts/package-msix.ts, which uses `readVersions()` to check all three agree).
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");
const TAURI_CONF = join(root, "src-tauri/tauri.conf.json");
const CARGO_TOML = join(root, "src-tauri/Cargo.toml");
const CARGO_LOCK = join(root, "src-tauri/Cargo.lock");
const PACKAGE_JSON = join(root, "package.json");

const VERSION_RE = /^\d+\.\d+\.\d+$/;
const CARGO_VERSION_RE = /^version = "([^"]*)"/m;
const LOCK_VERSION_RE = /^(name = "copet"\nversion = )"[^"]*"/m;

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
  const lock = readFileSync(CARGO_LOCK, "utf8");
  if (!LOCK_VERSION_RE.test(lock)) throw new Error("src-tauri/Cargo.lock has no copet package entry");
  writeFileSync(CARGO_LOCK, lock.replace(LOCK_VERSION_RE, `$1"${version}"`));
}

if (import.meta.main) {
  const version = process.argv[2];
  if (!version || !VERSION_RE.test(version) || version.split(".").some((n) => Number(n) > 65535)) {
    console.error("usage: bun run set-version <x.y.z> (numbers up to 65535, no prerelease suffix)");
    process.exit(1);
  }
  setVersion(version);
  console.log(`version ${version}:`, readVersions());
}
