// Compiles the `copet-mcp` sidecar to the file Tauri bundles as `externalBin` `binaries/copet-mcp`
// (`copet-mcp-<target triple>[.exe]`). The triple comes from Tauri (`TAURI_ENV_TARGET_TRIPLE`) or rustc.
import { rm } from "node:fs/promises";
import { $, Glob } from "bun";

const BUN_TARGET: Record<string, string> = {
  "aarch64-apple-darwin": "bun-darwin-arm64",
  "x86_64-pc-windows-msvc": "bun-windows-x64",
};
const triple = process.env.TAURI_ENV_TARGET_TRIPLE ?? (await $`rustc -vV`.text()).match(/^host: (\S+)$/m)?.[1];
if (!triple || !BUN_TARGET[triple]) throw new Error(`no bun target for ${triple}`);
const ext = triple.includes("windows") ? ".exe" : "";
await $`bun build --compile --target=${BUN_TARGET[triple]} src-mcp/main.ts --outfile src-tauri/binaries/copet-mcp-${triple}${ext}`;
// `--target` makes bun leave a copy of the target runtime in the working directory.
for await (const leftover of new Glob(".*.bun-build").scan({ dot: true })) await rm(leftover, { force: true });
