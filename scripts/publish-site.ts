// `COPET_STORE_ID=<id> COPET_SITE_REPO=<path> bun run publish:site`: copies site/copet/ over
// $COPET_SITE_REPO/copet/, replacing `{{STORE_URL}}` in HTML files with the Microsoft Store listing.
// Never runs git. Every failure exits 1 with a one-line reason, before anything is written.
import { mkdirSync, readdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join, relative } from "node:path";

const root = join(import.meta.dir, "..");
const SITE_DIR = join(root, "site/copet");

function fail(reason: string): never {
  console.error(reason);
  process.exit(1);
}

const storeId = process.env.COPET_STORE_ID || fail("missing env: COPET_STORE_ID (Partner Center › Product identity › Store ID, e.g. 9NXXXXXXXXXX)");
if (!/^9[A-Z0-9]{11}$/.test(storeId)) fail(`COPET_STORE_ID must be 12 characters starting with 9 (A-Z, 0-9): ${storeId}`);
const repo = process.env.COPET_SITE_REPO || fail("missing env: COPET_SITE_REPO");
if (!existsSync(repo)) fail(`COPET_SITE_REPO not found: ${repo}`);

const storeUrl = `https://apps.microsoft.com/detail/${storeId}`;
const files: { rel: string; data: string | Buffer }[] = [];
for (const e of readdirSync(SITE_DIR, { recursive: true, withFileTypes: true })) {
  if (!e.isFile()) continue;
  const path = join(e.parentPath, e.name);
  const rel = relative(SITE_DIR, path);
  if (!e.name.endsWith(".html")) {
    files.push({ rel, data: readFileSync(path) });
    continue;
  }
  const html = readFileSync(path, "utf8").replaceAll("{{STORE_URL}}", storeUrl);
  if (html.includes("{{")) fail(`unreplaced {{ token in site/copet/${rel}`);
  files.push({ rel, data: html });
}

const dest = join(repo, "copet");
for (const f of files) {
  const out = join(dest, f.rel);
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, f.data);
}
console.log(`Copied with Store link ${storeUrl}. Commit and push in ${repo}.`);
