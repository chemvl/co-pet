// Compiles the `copet-mcp` sidecar to the file Tauri bundles as `externalBin` `binaries/copet-mcp`.
import { rm } from "node:fs/promises";
import { $, Glob } from "bun";

await $`bun build --compile --target=bun-darwin-arm64 src-mcp/main.ts --outfile src-tauri/binaries/copet-mcp-aarch64-apple-darwin`;
// `--target` makes bun leave a copy of the target runtime in the working directory.
for await (const leftover of new Glob(".*.bun-build").scan({ dot: true })) await rm(leftover, { force: true });
