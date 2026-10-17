// `COPET_SITE_REPO=<path> bun run publish:site`: copies site/copet/ over $COPET_SITE_REPO/copet/.
// Never runs git. Every failure exits 1 with a one-line reason.
import { cpSync, existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");
const TAURI_CONF = join(root, "src-tauri/tauri.conf.json");
const SITE_DIR = join(root, "site/copet");

function fail(reason: string): never {
  console.error(reason);
  process.exit(1);
}

const repo = process.env.COPET_SITE_REPO || fail("missing env: COPET_SITE_REPO");
if (!existsSync(repo)) fail(`COPET_SITE_REPO not found: ${repo}`);

// Placeholder guard: owner literals from Task 7.1 must replace these before publishing.
const PLACEHOLDERS = ["OWNER_UPDATER_PUBKEY", "owner-domain.invalid"];
function guardFile(path: string) {
  const text = readFileSync(path, "utf8");
  for (const p of PLACEHOLDERS) if (text.includes(p)) fail(`placeholder ${p} in ${path.slice(root.length + 1)}`);
}
guardFile(TAURI_CONF);
for (const e of readdirSync(SITE_DIR, { recursive: true, withFileTypes: true }))
  if (e.isFile()) guardFile(join(e.parentPath, e.name));

const dest = join(repo, "copet");
cpSync(SITE_DIR, dest, { recursive: true });
console.log(`Copied. Commit and push in ${repo}.`);
