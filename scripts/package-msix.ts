// `bun run package:msix [--dev-cert]` (Windows only): builds Co-pet and packs it as the MSIX package
// submitted to the Microsoft Store, at src-tauri/target/release/msix/Co-pet_<x.y.z.0>_x64.msix.
//
// Steps: `fetch:llama`, `tauri build --no-bundle` (its beforeBuildCommand builds copet-mcp and the
// UI), stage copet.exe, the externalBin sidecars and the bundle resources of tauri.conf.json plus
// tauri.windows.conf.json the way Tauri lays them out (beside the exe), render the logo assets from
// src-tauri/icons/app-icon.png, fill in src-tauri/msix/AppxManifest.xml, index the assets into
// resources.pri (makepri, src-tauri/msix/priconfig.xml), then `makeappx pack`.
//
// Store build: needs COPET_MSIX_IDENTITY_NAME, COPET_MSIX_PUBLISHER and
// COPET_MSIX_PUBLISHER_DISPLAY_NAME (Partner Center › Product identity). The package stays unsigned;
// the Store signs it.
//
// `--dev-cert`: a sideloadable build. The identity defaults to `CoPet.Dev` / `CN=Co-pet Dev` unless
// the variables above are set. Signs with a self-signed code-signing certificate whose subject is the
// Publisher, created in Cert:\CurrentUser\My on first use and reused after; its public half is
// written to msix/Co-pet-dev.cer for the one-time trust command printed at the end.
//
// Needs the Windows SDK (makeappx, makepri, signtool from the newest Windows Kits\10\bin\<ver>\x64).
// Every failure exits 1 with a one-line reason.
import { Resvg } from "@resvg/resvg-js";
import { Glob } from "bun";
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { readVersions } from "./set-version";

const root = join(import.meta.dir, "..");
const TAURI_DIR = join(root, "src-tauri");
const RELEASE = join(TAURI_DIR, "target", "release");
const OUT = join(RELEASE, "msix");
const STAGE = join(OUT, "stage");
// Holds only Assets\: makepri indexes this directory, so no binary ends up in resources.pri.
const PRI_ROOT = join(OUT, "pri");
const TEMPLATES = join(TAURI_DIR, "msix");
const TARGET_TRIPLE = "x86_64-pc-windows-msvc";

function fail(reason: string): never {
  console.error(reason);
  process.exit(1);
}

function run(cmd: string[], opts: { capture?: boolean; env?: Record<string, string | undefined> } = {}): string {
  const p = Bun.spawnSync(cmd, {
    cwd: root,
    stdout: opts.capture ? "pipe" : "inherit",
    stderr: opts.capture ? "pipe" : "inherit",
    env: opts.env ?? process.env,
  });
  if (p.exitCode !== 0) {
    if (opts.capture) console.error(p.stdout.toString() + p.stderr.toString());
    fail(`failed: ${cmd.join(" ")}`);
  }
  return opts.capture ? p.stdout.toString() : "";
}

// --- 1. checks (before the long build)
if (process.platform !== "win32") fail(`package:msix runs on Windows only, not ${process.platform}`);
const args = process.argv.slice(2);
for (const a of args) if (a !== "--dev-cert") fail(`unknown argument ${a}; usage: package:msix [--dev-cert]`);
const devCert = args.includes("--dev-cert");

/** `key` from the environment; with --dev-cert, `devDefault` when unset. */
function setting(key: string, devDefault: string): string {
  const v = process.env[key];
  if (v) return v;
  if (devCert) return devDefault;
  return fail(`missing env: ${key} (Partner Center › Product identity), or pass --dev-cert for a local test package`);
}
const identityName = setting("COPET_MSIX_IDENTITY_NAME", "CoPet.Dev");
const publisher = setting("COPET_MSIX_PUBLISHER", "CN=Co-pet Dev");
const publisherDisplayName = setting("COPET_MSIX_PUBLISHER_DISPLAY_NAME", "Co-pet Dev");
if (!/^[A-Za-z0-9.-]{3,50}$/.test(identityName))
  fail(`COPET_MSIX_IDENTITY_NAME must be 3-50 characters of A-Z, a-z, 0-9, '.' and '-': ${identityName}`);
if (!publisher.startsWith("CN=")) fail(`COPET_MSIX_PUBLISHER must be a distinguished name starting with CN=: ${publisher}`);

const versions = readVersions();
const version = versions.tauri;
if (versions.cargo !== version || versions.npm !== version)
  fail(`versions differ: tauri ${versions.tauri}, cargo ${versions.cargo}, npm ${versions.npm}; run bun run set-version <x.y.z>`);
if (!/^\d+\.\d+\.\d+$/.test(version) || version.split(".").some((n) => Number(n) > 65535))
  fail(`version ${version} is not x.y.z with parts up to 65535, which MSIX needs`);
// The Store reserves the fourth part; it must be 0.
const msixVersion = `${version}.0`;

const KITS_BIN = join(process.env["ProgramFiles(x86)"] ?? "C:\\Program Files (x86)", "Windows Kits", "10", "bin");
/** Newest Windows SDK x64 `<name>` (`Windows Kits\10\bin\<version>\x64\<name>`). */
function sdkTool(name: string): string {
  const found = existsSync(KITS_BIN)
    ? readdirSync(KITS_BIN)
        .map((d) => join(KITS_BIN, d, "x64", name))
        .filter((p) => existsSync(p))
        .sort((a, b) => a.localeCompare(b, undefined, { numeric: true }))
    : [];
  return found.at(-1) ?? fail(`${name} not found under ${KITS_BIN}\\*\\x64; install the Windows SDK`);
}
const makeappx = sdkTool("makeappx.exe");
const makepri = sdkTool("makepri.exe");
const signtool = devCert ? sdkTool("signtool.exe") : undefined;

// --- 2. build
run(["bun", "run", "fetch:llama"]);
run(["bun", "run", "tauri", "build", "--no-bundle"]);

// --- 3. stage the files Tauri would install
rmSync(OUT, { recursive: true, force: true });
mkdirSync(STAGE, { recursive: true });

const placed = new Map<string, string>();
/** Copies `src` to `dest`, refusing two different sources for one destination. */
function place(src: string, dest: string) {
  const prev = placed.get(dest);
  if (prev && prev !== src) fail(`${prev} and ${src} both stage to ${dest}`);
  placed.set(dest, src);
  mkdirSync(dirname(dest), { recursive: true });
  cpSync(src, dest);
}

const exe = join(RELEASE, "copet.exe");
if (!existsSync(exe)) fail(`build output missing: ${exe}`);
place(exe, join(STAGE, "copet.exe"));

type BundleConf = { bundle?: { externalBin?: string[]; resources?: Record<string, string> | string[] } };
const [conf, winConf] = ["tauri.conf.json", "tauri.windows.conf.json"].map(
  (file) => JSON.parse(readFileSync(join(TAURI_DIR, file), "utf8")) as BundleConf,
);

// Sidecars sit beside the exe without the target triple (`binaries/copet-mcp` → `copet-mcp.exe`).
for (const bin of conf.bundle?.externalBin ?? []) {
  const src = join(TAURI_DIR, `${bin}-${TARGET_TRIPLE}.exe`);
  if (!existsSync(src)) fail(`sidecar missing: ${src}`);
  place(src, join(STAGE, `${basename(bin)}.exe`));
}

// Resource maps, `source (path or glob, relative to src-tauri)` → `target (relative to the exe)`;
// tauri.windows.conf.json merges into tauri.conf.json.
const resources: Record<string, string> = {};
for (const c of [conf, winConf]) {
  const r = c.bundle?.resources;
  if (Array.isArray(r)) fail("bundle.resources must be a source → target map");
  Object.assign(resources, r);
}
for (const [from, to] of Object.entries(resources)) {
  const targetDir = join(STAGE, to);
  if (/[*?[{]/.test(from)) {
    const matches = [...new Glob(from).scanSync({ cwd: TAURI_DIR, onlyFiles: true })];
    if (!matches.length) fail(`resource glob matched nothing: src-tauri/${from}`);
    for (const m of matches) place(join(TAURI_DIR, m), join(targetDir, basename(m)));
    continue;
  }
  const src = join(TAURI_DIR, from);
  if (!existsSync(src)) fail(`resource missing: src-tauri/${from}`);
  if (statSync(src).isDirectory()) {
    for (const rel of new Glob("**/*").scanSync({ cwd: src, onlyFiles: true, dot: true }))
      place(join(src, rel), join(targetDir, rel));
  } else {
    place(src, /[\\/]$/.test(to) || to === "." ? join(targetDir, basename(from)) : targetDir);
  }
}

// --- 4. logo assets: scale-* variants of every logo the manifest names, plus targetsize-* (plated
// and unplated) of the app-list / taskbar icon. Tiles keep a margin around the icon.
const icon = readFileSync(join(TAURI_DIR, "icons", "app-icon.png")).toString("base64");
const assetsDir = join(PRI_ROOT, "Assets");
mkdirSync(assetsDir, { recursive: true });

function renderAsset(file: string, w: number, h: number, fill: number) {
  const size = Math.round(Math.min(w, h) * fill);
  const x = Math.floor((w - size) / 2);
  const y = Math.floor((h - size) / 2);
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">` +
    `<image x="${x}" y="${y}" width="${size}" height="${size}" xlink:href="data:image/png;base64,${icon}"/></svg>`;
  const png = new Resvg(svg, { fitTo: { mode: "original" }, background: "rgba(0,0,0,0)" }).render().asPng();
  writeFileSync(join(assetsDir, file), png);
}

const SCALES = [100, 125, 150, 200, 400];
const LOGOS = [
  { name: "StoreLogo", w: 50, h: 50, fill: 1 },
  { name: "Square44x44Logo", w: 44, h: 44, fill: 1 },
  { name: "Square71x71Logo", w: 71, h: 71, fill: 0.6 },
  { name: "Square150x150Logo", w: 150, h: 150, fill: 0.6 },
  { name: "Wide310x150Logo", w: 310, h: 150, fill: 0.6 },
  { name: "Square310x310Logo", w: 310, h: 310, fill: 0.6 },
];
for (const { name, w, h, fill } of LOGOS)
  for (const s of SCALES) renderAsset(`${name}.scale-${s}.png`, Math.round((w * s) / 100), Math.round((h * s) / 100), fill);
for (const t of [16, 24, 32, 48, 256]) {
  renderAsset(`Square44x44Logo.targetsize-${t}.png`, t, t, 1);
  renderAsset(`Square44x44Logo.targetsize-${t}_altform-unplated.png`, t, t, 1);
}
cpSync(assetsDir, join(STAGE, "Assets"), { recursive: true });

// --- 5. manifest
const escapeXml = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&apos;");
const values: Record<string, string> = {
  IDENTITY_NAME: identityName,
  PUBLISHER: publisher,
  PUBLISHER_DISPLAY_NAME: publisherDisplayName,
  VERSION: msixVersion,
};
const manifestPath = join(STAGE, "AppxManifest.xml");
writeFileSync(
  manifestPath,
  readFileSync(join(TEMPLATES, "AppxManifest.xml"), "utf8").replace(/\{\{(\w+)\}\}/g, (_, k: string) =>
    escapeXml(values[k] ?? fail(`unknown placeholder {{${k}}} in src-tauri/msix/AppxManifest.xml`)),
  ),
);

// --- 6. resources.pri, then the package
run([
  makepri, "new",
  "/pr", PRI_ROOT,
  "/cf", join(TEMPLATES, "priconfig.xml"),
  "/mn", manifestPath,
  "/of", join(STAGE, "resources.pri"),
  "/o",
]);
const msix = join(OUT, `Co-pet_${msixVersion}_x64.msix`);
run([makeappx, "pack", "/d", STAGE, "/p", msix, "/o"]);

// --- 7. dev signing
if (!devCert) {
  console.log(`built ${msix}\nunsigned: upload it to Partner Center, which signs it for the Store`);
  process.exit(0);
}

const cer = join(OUT, "Co-pet-dev.cer");
// Reuses a valid code-signing certificate with the Publisher as subject, else creates one
// (https://learn.microsoft.com/windows/msix/package/create-certificate-package-signing).
const CERT_SCRIPT = `
$ErrorActionPreference = 'Stop'
$subject = $env:COPET_CERT_SUBJECT
$cert = Get-ChildItem Cert:\\CurrentUser\\My -CodeSigningCert |
  Where-Object { $_.Subject -eq $subject -and $_.NotAfter -gt (Get-Date).AddDays(1) } |
  Sort-Object NotAfter -Descending | Select-Object -First 1
if (-not $cert) {
  $cert = New-SelfSignedCertificate -Type Custom -Subject $subject -KeyUsage DigitalSignature \`
    -FriendlyName 'Co-pet MSIX dev signing' -CertStoreLocation Cert:\\CurrentUser\\My \`
    -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}')
}
Export-Certificate -Cert $cert -FilePath $env:COPET_CERT_FILE -Force | Out-Null
Write-Output $cert.Thumbprint
`;
const thumbprint =
  run(["powershell.exe", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", CERT_SCRIPT], {
    capture: true,
    env: { ...process.env, COPET_CERT_SUBJECT: publisher, COPET_CERT_FILE: cer },
  })
    .trim()
    .split(/\r?\n/)
    .at(-1)
    ?.trim() ?? "";
if (!/^[0-9A-F]{40}$/i.test(thumbprint)) fail(`could not get a signing certificate for ${publisher}: ${thumbprint}`);
run([signtool!, "sign", "/fd", "SHA256", "/s", "My", "/sha1", thumbprint, msix]);

console.log(`built and signed ${msix} (certificate ${publisher}, ${thumbprint})

Once per machine, trust the certificate from an administrator PowerShell:
  Import-Certificate -FilePath "${cer}" -CertStoreLocation Cert:\\LocalMachine\\TrustedPeople
Then install:
  Add-AppxPackage "${msix}"`);
