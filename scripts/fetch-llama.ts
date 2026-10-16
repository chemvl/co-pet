// Fetches the pinned llama.cpp macOS arm64 build and copies llama-server plus the dylibs it
// loads into src-tauri/resources/llama/ (bundled as Contents/Resources/llama/).
// Idempotent: skips everything when resources/llama/.version already equals the pinned build.
import { createHash } from "node:crypto";
import { chmod, copyFile, mkdir, mkdtemp, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const BUILD = "b11669";
const URL = `https://github.com/ggml-org/llama.cpp/releases/download/${BUILD}/llama-${BUILD}-bin-macos-arm64.tar.gz`;
const SHA256 = "dbed55493d72bc1ba2babc8770d669c2289b5bc4ee9fb2a4dfc158d45b7d13fd";
const FILES = [
  "llama-server",
  "libllama-server-impl.dylib",
  "libllama-common.0.dylib",
  "libmtmd.0.dylib",
  "libllama.0.dylib",
  "libggml.0.dylib",
  "libggml-cpu.0.dylib",
  "libggml-blas.0.dylib",
  "libggml-metal.0.dylib",
  "libggml-rpc.0.dylib",
  "libggml-base.0.dylib",
  "LICENSE",
];

const root = resolve(import.meta.dir, "..");
const dest = join(root, "src-tauri", "resources", "llama");
const versionFile = join(dest, ".version");

async function exists(p: string): Promise<boolean> {
  try {
    await stat(p);
    return true;
  } catch {
    return false;
  }
}

/** Finds `name` anywhere under `dir` (the archive layout nests a build folder). */
async function findFile(dir: string, name: string): Promise<string | null> {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const p = join(dir, entry.name);
    if (entry.name === name && !entry.isDirectory()) return p;
    if (entry.isDirectory()) {
      const found = await findFile(p, name);
      if (found) return found;
    }
  }
  return null;
}

if ((await exists(versionFile)) && (await readFile(versionFile, "utf8")).trim() === BUILD) {
  console.log(`llama.cpp ${BUILD} already present in ${dest}`);
  process.exit(0);
}

const work = await mkdtemp(join(tmpdir(), "copet-llama-"));
try {
  console.log(`Downloading ${URL}`);
  const res = await fetch(URL);
  if (!res.ok) throw new Error(`download failed: HTTP ${res.status}`);
  const bytes = new Uint8Array(await res.arrayBuffer());
  const digest = createHash("sha256").update(bytes).digest("hex");
  if (digest !== SHA256) throw new Error(`checksum mismatch: got ${digest}, expected ${SHA256}`);
  const archive = join(work, "llama.tar.gz");
  await writeFile(archive, bytes);

  const tar = Bun.spawn(["tar", "-xzf", archive, "-C", work], { stdout: "inherit", stderr: "inherit" });
  if ((await tar.exited) !== 0) throw new Error("tar -xzf failed");

  await rm(dest, { recursive: true, force: true });
  await mkdir(dest, { recursive: true });
  for (const name of FILES) {
    const src = await findFile(work, name);
    if (!src) throw new Error(`${name} not found in the archive`);
    // copyFile follows symlinks, so every copied file is a real file.
    await copyFile(src, join(dest, name));
  }
  await chmod(join(dest, "llama-server"), 0o755);
  await writeFile(versionFile, `${BUILD}\n`);
  console.log(`Installed llama.cpp ${BUILD} into ${dest}`);
} finally {
  await rm(work, { recursive: true, force: true });
}
