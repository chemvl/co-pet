// `bun --env-file=.env.prod run release -- --notes <file> [--prefix copet|copet-test]`:
// checks, signed + notarized Tauri build, DMG sign/notarize/staple, verify, upload to R2, tag.
// Every failure exits 1 with a one-line reason.
import { existsSync, readdirSync, readFileSync, writeFileSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { readVersions } from "./set-version";

const root = join(import.meta.dir, "..");
const TAURI_CONF = join(root, "src-tauri/tauri.conf.json");
const SITE_DIR = join(root, "site/copet");
const BUNDLE = join(root, "src-tauri/target/release/bundle");

function fail(reason: string): never {
  console.error(reason);
  process.exit(1);
}

function run(cmd: string[], opts: { capture?: boolean } = {}): string {
  const p = Bun.spawnSync(cmd, {
    cwd: root,
    stdout: opts.capture ? "pipe" : "inherit",
    stderr: opts.capture ? "pipe" : "inherit",
    env: process.env,
  });
  const out = opts.capture ? p.stdout.toString() + p.stderr.toString() : "";
  if (p.exitCode !== 0) fail(`failed: ${cmd.join(" ")}`);
  return out;
}

// --- args
const args = process.argv.slice(2);
function arg(name: string): string | undefined {
  const i = args.indexOf(name);
  if (i === -1) return undefined;
  const v = args[i + 1];
  if (!v || v.startsWith("--")) fail(`${name} needs a value`);
  return v;
}
const notesPath = arg("--notes") ?? fail("usage: release -- --notes <file> [--prefix copet|copet-test]");
const prefix = arg("--prefix") ?? "copet";
if (prefix !== "copet" && prefix !== "copet-test") fail(`--prefix must be copet or copet-test, got ${prefix}`);
if (!existsSync(notesPath)) fail(`notes file not found: ${notesPath}`);
const notes = readFileSync(notesPath, "utf8");

// --- 1. checks
const REQUIRED = [
  "APPLE_SIGNING_IDENTITY",
  "APPLE_API_ISSUER",
  "APPLE_API_KEY",
  "APPLE_API_KEY_PATH",
  "TAURI_SIGNING_PRIVATE_KEY",
  "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
  "COPET_SUPABASE_URL",
  "COPET_SUPABASE_KEY",
  "COPET_R2_BUCKET",
  "COPET_UPDATE_HOST",
] as const;
const missing = REQUIRED.filter((k) => !process.env[k]);
if (missing.length) fail(`missing env: ${missing.join(" ")}`);
const env = Object.fromEntries(REQUIRED.map((k) => [k, process.env[k]!])) as Record<(typeof REQUIRED)[number], string>;

// Placeholder guard: owner literals from Task 7.1 must replace these before any release.
const PLACEHOLDERS = ["OWNER_UPDATER_PUBKEY", "owner-domain.invalid"];
function guardFile(path: string) {
  const text = readFileSync(path, "utf8");
  for (const p of PLACEHOLDERS) if (text.includes(p)) fail(`placeholder ${p} in ${path.slice(root.length + 1)}`);
}
guardFile(TAURI_CONF);
if (existsSync(SITE_DIR)) {
  for (const e of readdirSync(SITE_DIR, { recursive: true, withFileTypes: true }))
    if (e.isFile()) guardFile(join(e.parentPath, e.name));
}

const conf = JSON.parse(readFileSync(TAURI_CONF, "utf8"));
const endpoint: string | undefined = conf.plugins?.updater?.endpoints?.[0];
if (!endpoint) fail("tauri.conf.json has no plugins.updater.endpoints");
if (new URL(endpoint).host !== env.COPET_UPDATE_HOST)
  fail(`COPET_UPDATE_HOST ${env.COPET_UPDATE_HOST} differs from updater endpoint host ${new URL(endpoint).host}`);

if (run(["git", "status", "--porcelain"], { capture: true }).trim()) fail("git tree is dirty; commit or stash first");

const versions = readVersions();
const v = versions.tauri;
if (versions.cargo !== v || versions.npm !== v)
  fail(`versions differ: tauri ${versions.tauri}, cargo ${versions.cargo}, npm ${versions.npm}`);

const feed = `https://${env.COPET_UPDATE_HOST}/${prefix}/latest.json`;
const res = await fetch(feed).catch((e) => fail(`fetch ${feed}: ${e.message}`));
if (res.status !== 404) {
  if (!res.ok) fail(`fetch ${feed}: HTTP ${res.status}`);
  const published = (await res.json()) as { version: string };
  if (Bun.semver.order(v, published.version) !== 1) fail(`version ${v} is not newer than published ${published.version}`);
}

// --- 2. build
if (prefix !== "copet") process.env.COPET_UPDATE_FEED = feed;
run(["bun", "run", "fetch:llama"]);
run(["bun", "run", "sign:llama"]);
const confOverride = join(mkdtempSync(join(tmpdir(), "copet-release-")), "sign.json");
writeFileSync(confOverride, JSON.stringify({ bundle: { createUpdaterArtifacts: true, macOS: { signingIdentity: env.APPLE_SIGNING_IDENTITY } } }));
run(["bun", "run", "tauri", "build", "--config", confOverride]);

const app = join(BUNDLE, "macos/Co-pet.app");
const tarball = join(BUNDLE, "macos/Co-pet.app.tar.gz");
const sigPath = `${tarball}.sig`;
const dmg = join(BUNDLE, `dmg/Co-pet_${v}_aarch64.dmg`);
for (const f of [app, tarball, sigPath, dmg]) if (!existsSync(f)) fail(`build output missing: ${f}`);

// --- 3. DMG
run(["codesign", "--force", "--timestamp", "-s", env.APPLE_SIGNING_IDENTITY, dmg]);
run([
  "xcrun", "notarytool", "submit", dmg,
  "--key", env.APPLE_API_KEY_PATH, "--key-id", env.APPLE_API_KEY, "--issuer", env.APPLE_API_ISSUER, "--wait",
]);
run(["xcrun", "stapler", "staple", dmg]);

// --- 4. verify
run(["codesign", "--verify", "--deep", "--strict", app]);
if (!run(["spctl", "-a", "-vv", app], { capture: true }).includes("source=Notarized Developer ID"))
  fail("spctl: app is not source=Notarized Developer ID");
run(["spctl", "-a", "-t", "open", "--context", "context:primary-signature", "-vv", dmg]);
run(["xcrun", "stapler", "validate", app]);
run(["xcrun", "stapler", "validate", dmg]);

// --- 5. upload (latest.json last, so clients never see a feed pointing at missing files)
const IMMUTABLE = "public, max-age=31536000, immutable";
function put(key: string, file: string, extra: string[]) {
  run(["bunx", "wrangler", "r2", "object", "put", `${env.COPET_R2_BUCKET}/${key}`, "--file", file, "--remote", ...extra]);
}
const tarKey = `${prefix}/${v}/Co-pet_${v}_aarch64.app.tar.gz`;
put(tarKey, tarball, ["--cache-control", IMMUTABLE]);
put(`${prefix}/${v}/Co-pet_${v}_aarch64.dmg`, dmg, ["--cache-control", IMMUTABLE]);
put(`${prefix}/Co-pet.dmg`, dmg, ["--cache-control", "max-age=300"]);
const latest = {
  version: v,
  notes,
  pub_date: new Date().toISOString(),
  platforms: {
    "darwin-aarch64": {
      signature: readFileSync(sigPath, "utf8"),
      url: `https://${env.COPET_UPDATE_HOST}/${tarKey}`,
    },
  },
};
const latestPath = join(confOverride, "..", "latest.json");
writeFileSync(latestPath, JSON.stringify(latest, null, 2));
put(`${prefix}/latest.json`, latestPath, ["--cache-control", "max-age=300", "--content-type", "application/json"]);

// --- 6. tag
if (prefix === "copet") {
  run(["git", "tag", `v${v}`]);
  run(["git", "push", "origin", `v${v}`]);
}
console.log(`released ${v} to ${prefix}`);
