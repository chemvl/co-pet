// Signs the bundled llama.cpp binaries in src-tauri/resources/llama/.
// macOS: with the Developer ID (hardened runtime + secure timestamp), dylibs first and
// llama-server last, then verifies each.
// Windows: every .exe/.dll with Azure Trusted Signing (`trusted-signing-cli`). Skipped when
// COPET_TS_ENDPOINT is unset: internal builds may be unsigned.
import { existsSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";

const dir = join(resolve(import.meta.dir, ".."), "src-tauri", "resources", "llama");

/** The file names in `dir`; exits when `bun run fetch:llama` has not filled it. */
function runtimeFiles(): string[] {
  if (!existsSync(join(dir, ".version"))) {
    console.error("run bun run fetch:llama first");
    process.exit(1);
  }
  return readdirSync(dir);
}

function signWindows(): void {
  const endpoint = process.env.COPET_TS_ENDPOINT;
  if (!endpoint) {
    console.log("COPET_TS_ENDPOINT is not set: leaving the llama.cpp binaries unsigned");
    return;
  }
  const account = process.env.COPET_TS_ACCOUNT;
  const profile = process.env.COPET_TS_PROFILE;
  if (!account || !profile) {
    console.error("COPET_TS_ACCOUNT and COPET_TS_PROFILE must be set with COPET_TS_ENDPOINT");
    process.exit(1);
  }
  const files = runtimeFiles()
    .filter((n) => n.endsWith(".exe") || n.endsWith(".dll"))
    .sort()
    .map((n) => join(dir, n));
  for (const file of files) {
    const p = Bun.spawnSync(
      ["trusted-signing-cli", "-e", endpoint, "-a", account, "-c", profile, "-d", "Co-pet", file],
      { stdout: "inherit", stderr: "inherit" },
    );
    if (p.exitCode !== 0) {
      console.error(`trusted-signing-cli failed: ${file}`);
      process.exit(1);
    }
  }
  console.log(`signed ${files.length} files in ${dir}`);
}

function signMac(): void {
  const identity = process.env.APPLE_SIGNING_IDENTITY;
  if (!identity) {
    console.error("APPLE_SIGNING_IDENTITY is not set");
    process.exit(1);
  }

  const names = runtimeFiles();
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
}

if (process.platform === "win32") signWindows();
else signMac();
