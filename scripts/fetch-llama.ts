// Fetches the pinned llama.cpp build for this OS (macOS arm64, Windows x64 Vulkan) and copies
// llama-server plus the libraries it loads into src-tauri/resources/llama/ (bundled as
// Contents/Resources/llama/ on macOS, resources\llama\ on Windows).
// Idempotent: skips everything when resources/llama/.version already equals `<build>-<platform>`.
import { createHash } from "node:crypto";
import { chmod, copyFile, mkdir, mkdtemp, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";

const BUILD = "b11669";
const TARGETS = {
  darwin: {
    asset: `llama-${BUILD}-bin-macos-arm64.tar.gz`,
    sha256: "dbed55493d72bc1ba2babc8770d669c2289b5bc4ee9fb2a4dfc158d45b7d13fd",
    files: [
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
    ],
    exe: "llama-server",
  },
  win32: {
    asset: `llama-${BUILD}-bin-win-vulkan-x64.zip`,
    sha256: "2c07574e7bde71321cb785282b20cc821e951132149eeceeea32ddebdbf0598e",
    // Empty: llama-server.exe, the DLLs beside it (ggml loads its CPU/Vulkan backends at runtime)
    // without the other tools' `*-impl.dll`, and the LICENSE files.
    files: [],
    exe: "llama-server.exe",
  },
} as const;
const target =
  TARGETS[process.platform as keyof typeof TARGETS] ??
  (() => {
    throw new Error(`unsupported platform ${process.platform}`);
  })();
const URL = `https://github.com/ggml-org/llama.cpp/releases/download/${BUILD}/${target.asset}`;
const VERSION = `${BUILD}-${process.platform}`;

const root = resolve(import.meta.dir, "..");
const dest = join(root, "src-tauri", "resources", "llama");
const versionFile = join(dest, ".version");

/** Windows: whether `name`, beside llama-server.exe, is needed to run it. */
function windowsRuntimeFile(name: string): boolean {
  if (name === target.exe || name.startsWith("LICENSE")) return true;
  return name.endsWith(".dll") && (!name.endsWith("-impl.dll") || name === "llama-server-impl.dll");
}

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

if ((await exists(versionFile)) && (await readFile(versionFile, "utf8")).trim() === VERSION) {
  console.log(`llama.cpp ${VERSION} already present in ${dest}`);
  process.exit(0);
}

const work = await mkdtemp(join(tmpdir(), "copet-llama-"));
try {
  console.log(`Downloading ${URL}`);
  const res = await fetch(URL);
  if (!res.ok) throw new Error(`download failed: HTTP ${res.status}`);
  const bytes = new Uint8Array(await res.arrayBuffer());
  const digest = createHash("sha256").update(bytes).digest("hex");
  if (digest !== target.sha256) throw new Error(`checksum mismatch: got ${digest}, expected ${target.sha256}`);
  const archive = join(work, target.asset);
  await writeFile(archive, bytes);

  // bsdtar reads both .tar.gz and .zip. On Windows, System32's bsdtar, not a Git Bash GNU tar on PATH.
  const tarExe = process.platform === "win32" ? join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe") : "tar";
  const tar = Bun.spawn([tarExe, "-xf", archive, "-C", work], { stdout: "inherit", stderr: "inherit" });
  if ((await tar.exited) !== 0) throw new Error("tar -xf failed");

  const exe = await findFile(work, target.exe);
  if (!exe) throw new Error(`${target.exe} not found in the archive`);
  const files: readonly string[] = target.files.length
    ? target.files
    : (await readdir(dirname(exe))).filter(windowsRuntimeFile);

  await rm(dest, { recursive: true, force: true });
  await mkdir(dest, { recursive: true });
  for (const name of files) {
    const src = await findFile(work, name);
    if (!src) throw new Error(`${name} not found in the archive`);
    // copyFile follows symlinks, so every copied file is a real file.
    await copyFile(src, join(dest, name));
  }
  if (process.platform !== "win32") await chmod(join(dest, target.exe), 0o755);
  await writeFile(versionFile, `${VERSION}\n`);
  console.log(`Installed llama.cpp ${VERSION} into ${dest}`);
} finally {
  await rm(work, { recursive: true, force: true });
}
