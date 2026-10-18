// `bun --env-file=.env.prod run release -- --notes <file> [--prefix copet|copet-test]`, on macOS or Windows:
// checks, signed Tauri build (macOS: notarized, plus DMG sign/notarize/staple; Windows: Authenticode
// through Azure Trusted Signing), verify, upload this platform's artifacts and `<prefix>/<v>/<platform>.json`
// to R2. Once both platforms' fragments exist for the version, publish `latest.json` and tag.
// Every failure exits 1 with a one-line reason.
import { existsSync, readdirSync, readFileSync, writeFileSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { readVersions } from "./set-version";

const root = join(import.meta.dir, "..");
const TAURI_CONF = join(root, "src-tauri/tauri.conf.json");
const SITE_DIR = join(root, "site/copet");
const BUNDLE = join(root, "src-tauri/target/release/bundle");
const WINDOWS_KITS_BIN = "C:\\Program Files (x86)\\Windows Kits\\10\\bin";

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

// --- feed logic (pure)
const PLATFORMS = ["darwin-aarch64", "windows-x86_64"] as const;
type Platform = (typeof PLATFORMS)[number];
type Fragment = { signature: string; url: string };

/** Why `v` can't be released over the published version, or undefined when it can. `v` may equal the
 * published version only to add this platform's build next to the other platform's. */
function versionError(
  v: string,
  published: string,
  platform: Platform,
  hasThis: boolean,
  hasOther: boolean,
): string | undefined {
  const order = Bun.semver.order(v, published);
  if (order === 1) return undefined;
  if (order === 0 && hasOther && !hasThis) return undefined;
  if (order === 0)
    return hasThis
      ? `version ${v} is already published for ${platform}`
      : `version ${v} equals published ${published} and has no build for the other platform`;
  return `version ${v} is not newer than published ${published}`;
}

/** The `latest.json` feed, or undefined while a platform's fragment is missing. */
function latestFeed(v: string, notes: string, pubDate: string, fragments: Partial<Record<Platform, Fragment>>) {
  const darwin = fragments["darwin-aarch64"];
  const windows = fragments["windows-x86_64"];
  if (!darwin || !windows) return undefined;
  return {
    version: v,
    notes,
    pub_date: pubDate,
    platforms: {
      "darwin-aarch64": { signature: darwin.signature, url: darwin.url },
      "windows-x86_64": { signature: windows.signature, url: windows.url },
    },
  };
}
// --- end feed logic

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
if (process.platform !== "darwin" && process.platform !== "win32") fail(`release runs on macOS or Windows, not ${process.platform}`);
const PLATFORM: Platform = process.platform === "win32" ? "windows-x86_64" : "darwin-aarch64";
const OTHER: Platform = PLATFORM === "windows-x86_64" ? "darwin-aarch64" : "windows-x86_64";

const SHARED_ENV = [
  "TAURI_SIGNING_PRIVATE_KEY",
  "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
  "COPET_SUPABASE_URL",
  "COPET_SUPABASE_KEY",
  "COPET_R2_BUCKET",
  "COPET_UPDATE_HOST",
] as const;
const DARWIN_ENV = ["APPLE_SIGNING_IDENTITY", "APPLE_API_ISSUER", "APPLE_API_KEY", "APPLE_API_KEY_PATH"] as const;
const WIN32_ENV = [
  "COPET_TS_ENDPOINT",
  "COPET_TS_ACCOUNT",
  "COPET_TS_PROFILE",
  "AZURE_CLIENT_ID",
  "AZURE_CLIENT_SECRET",
  "AZURE_TENANT_ID",
] as const;
type EnvKey = (typeof SHARED_ENV | typeof DARWIN_ENV | typeof WIN32_ENV)[number];
const REQUIRED: readonly EnvKey[] = [...(PLATFORM === "windows-x86_64" ? WIN32_ENV : DARWIN_ENV), ...SHARED_ENV];
const missing = REQUIRED.filter((k) => !process.env[k]);
if (missing.length) fail(`missing env: ${missing.join(" ")}`);
// Only the keys required on this platform are set.
const env = Object.fromEntries(REQUIRED.map((k) => [k, process.env[k]!])) as Record<EnvKey, string>;

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

async function get(url: string): Promise<Response> {
  return fetch(url).catch((e) => fail(`fetch ${url}: ${e.message}`));
}
const fragmentKey = (p: Platform) => `${prefix}/${v}/${p}.json`;
/** This version's fragment for `p`: parsed on 200, undefined on 404; anything else fails. The query
 * string bypasses any CDN copy, including a cached 404 from before the upload. */
async function fetchFragment(p: Platform): Promise<Fragment | undefined> {
  const url = `https://${env.COPET_UPDATE_HOST}/${fragmentKey(p)}?${Date.now()}`;
  const r = await get(url);
  if (r.status === 404) return undefined;
  if (r.status !== 200) fail(`fetch ${url}: HTTP ${r.status}`);
  return (await r.json()) as Fragment;
}

const feed = `https://${env.COPET_UPDATE_HOST}/${prefix}/latest.json`;
const res = await get(feed);
if (res.status !== 404) {
  if (!res.ok) fail(`fetch ${feed}: HTTP ${res.status}`);
  const published = (await res.json()) as { version: string };
  // Fragments matter only when `v` equals the published version.
  const same = Bun.semver.order(v, published.version) === 0;
  const hasThis = same && (await fetchFragment(PLATFORM)) !== undefined;
  const hasOther = same && (await fetchFragment(OTHER)) !== undefined;
  const error = versionError(v, published.version, PLATFORM, hasThis, hasOther);
  if (error) fail(error);
}

// --- 2. build
if (prefix !== "copet") process.env.COPET_UPDATE_FEED = feed;
run(["bun", "run", "fetch:llama"]);
run(["bun", "run", "sign:llama"]);
const confOverride = join(mkdtempSync(join(tmpdir(), "copet-release-")), "sign.json");
const bundleOverride =
  PLATFORM === "windows-x86_64"
    ? {
        createUpdaterArtifacts: true,
        windows: {
          signCommand: {
            cmd: "trusted-signing-cli",
            args: ["-e", env.COPET_TS_ENDPOINT, "-a", env.COPET_TS_ACCOUNT, "-c", env.COPET_TS_PROFILE, "-d", "Co-pet", "%1"],
          },
        },
      }
    : { createUpdaterArtifacts: true, macOS: { signingIdentity: env.APPLE_SIGNING_IDENTITY } };
writeFileSync(confOverride, JSON.stringify({ bundle: bundleOverride }));
run(["bun", "run", "tauri", "build", "--config", confOverride]);

// --- 3-5. platform artifacts: verify, then upload (the fragment after them, latest.json last, so
// clients never see a feed pointing at missing files)
const IMMUTABLE = "public, max-age=31536000, immutable";
function put(key: string, file: string, extra: string[]) {
  run(["bunx", "wrangler", "r2", "object", "put", `${env.COPET_R2_BUCKET}/${key}`, "--file", file, "--remote", ...extra]);
}

/** Newest Windows SDK x64 `signtool.exe` (`C:\Program Files (x86)\Windows Kits\10\bin\*\x64\signtool.exe`). */
function findSigntool(): string {
  const found = existsSync(WINDOWS_KITS_BIN)
    ? readdirSync(WINDOWS_KITS_BIN)
        .map((d) => join(WINDOWS_KITS_BIN, d, "x64", "signtool.exe"))
        .filter((p) => existsSync(p))
        .sort((a, b) => a.localeCompare(b, undefined, { numeric: true }))
    : [];
  return found.at(-1) ?? fail(`signtool.exe not found under ${WINDOWS_KITS_BIN}\\*\\x64; install the Windows SDK`);
}

function releaseWindows(): Fragment {
  const exe = join(BUNDLE, `nsis/Co-pet_${v}_x64-setup.exe`);
  const sigPath = `${exe}.sig`;
  for (const f of [exe, sigPath]) if (!existsSync(f)) fail(`build output missing: ${f}`);

  run([findSigntool(), "verify", "/pa", "/v", exe]);

  const exeKey = `${prefix}/${v}/Co-pet_${v}_x64-setup.exe`;
  put(exeKey, exe, ["--cache-control", IMMUTABLE]);
  put(`${prefix}/Co-pet-setup.exe`, exe, ["--cache-control", "max-age=300"]);
  return { signature: readFileSync(sigPath, "utf8"), url: `https://${env.COPET_UPDATE_HOST}/${exeKey}` };
}

function releaseMac(): Fragment {
  const app = join(BUNDLE, "macos/Co-pet.app");
  const tarball = join(BUNDLE, "macos/Co-pet.app.tar.gz");
  const sigPath = `${tarball}.sig`;
  const dmg = join(BUNDLE, `dmg/Co-pet_${v}_aarch64.dmg`);
  for (const f of [app, tarball, sigPath, dmg]) if (!existsSync(f)) fail(`build output missing: ${f}`);

  // DMG
  run(["codesign", "--force", "--timestamp", "-s", env.APPLE_SIGNING_IDENTITY, dmg]);
  run([
    "xcrun", "notarytool", "submit", dmg,
    "--key", env.APPLE_API_KEY_PATH, "--key-id", env.APPLE_API_KEY, "--issuer", env.APPLE_API_ISSUER, "--wait",
  ]);
  run(["xcrun", "stapler", "staple", dmg]);

  // verify
  run(["codesign", "--verify", "--deep", "--strict", app]);
  if (!run(["spctl", "-a", "-vv", app], { capture: true }).includes("source=Notarized Developer ID"))
    fail("spctl: app is not source=Notarized Developer ID");
  run(["spctl", "-a", "-t", "open", "--context", "context:primary-signature", "-vv", dmg]);
  run(["xcrun", "stapler", "validate", app]);
  run(["xcrun", "stapler", "validate", dmg]);

  // upload
  const tarKey = `${prefix}/${v}/Co-pet_${v}_aarch64.app.tar.gz`;
  put(tarKey, tarball, ["--cache-control", IMMUTABLE]);
  put(`${prefix}/${v}/Co-pet_${v}_aarch64.dmg`, dmg, ["--cache-control", IMMUTABLE]);
  put(`${prefix}/Co-pet.dmg`, dmg, ["--cache-control", "max-age=300"]);
  return { signature: readFileSync(sigPath, "utf8"), url: `https://${env.COPET_UPDATE_HOST}/${tarKey}` };
}

const fragment = PLATFORM === "windows-x86_64" ? releaseWindows() : releaseMac();
const fragmentPath = join(confOverride, "..", `${PLATFORM}.json`);
writeFileSync(fragmentPath, JSON.stringify(fragment, null, 2));
put(fragmentKey(PLATFORM), fragmentPath, ["--cache-control", "no-store", "--content-type", "application/json"]);

// --- 6. publish, once both platforms' fragments are up
const fragments: Partial<Record<Platform, Fragment>> = {};
for (const p of PLATFORMS) fragments[p] = await fetchFragment(p);
const latest = latestFeed(v, notes, new Date().toISOString(), fragments);
if (!latest) {
  console.log(`waiting for ${PLATFORMS.filter((p) => !fragments[p]).join(" and ")} build of ${v}`);
  process.exit(0);
}
const latestPath = join(confOverride, "..", "latest.json");
writeFileSync(latestPath, JSON.stringify(latest, null, 2));
put(`${prefix}/latest.json`, latestPath, ["--cache-control", "max-age=300", "--content-type", "application/json"]);

// --- 7. tag
if (prefix === "copet") {
  run(["git", "tag", `v${v}`]);
  run(["git", "push", "origin", `v${v}`]);
}
console.log(`released ${v} to ${prefix}`);
