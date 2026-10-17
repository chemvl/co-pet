// `bun --env-file=.env.prod scripts/build-local.ts` (= `bun run build:prod`): the `.app` bundle,
// signed with a stable certificate from the login keychain instead of the ad-hoc "-" signature.
//
// Why: an ad-hoc signature's designated requirement is the binary's hash, so every rebuild is a
// new app to the keychain and macOS asks for the login password again for every Co-pet secret.
// With a certificate the requirement is "identifier com.copet.app + that certificate", which
// survives rebuilds: click "Always Allow" once per entry and later builds read it silently.
//
// Identity: $APPLE_SIGNING_IDENTITY, else the first "Developer ID Application" or
// "Apple Development" identity from `security find-identity`. With none, falls back to ad-hoc.
// Not notarized: the Apple notarization env vars are dropped for this build.
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = join(import.meta.dir, "..");

function findIdentity(): string | undefined {
  const p = Bun.spawnSync(["security", "find-identity", "-v", "-p", "codesigning"]);
  const names = [...p.stdout.toString().matchAll(/^\s*\d+\)\s+[0-9A-F]{40}\s+"(.+)"$/gm)].map((m) => m[1]);
  return (
    names.find((n) => n.startsWith("Developer ID Application:")) ??
    names.find((n) => n.startsWith("Apple Development:"))
  );
}

const identity = process.env.APPLE_SIGNING_IDENTITY || findIdentity();
if (identity) {
  console.log(`signing with "${identity}"`);
} else {
  console.warn(
    "no code signing identity found: building ad-hoc. macOS will ask for the keychain password " +
      "after every rebuild. Create an \"Apple Development\" certificate in Xcode › Settings › Accounts to avoid it.",
  );
}

const conf = join(mkdtempSync(join(tmpdir(), "copet-build-")), "sign.json");
writeFileSync(conf, JSON.stringify({ bundle: { macOS: { signingIdentity: identity ?? "-" } } }));

const env = { ...process.env };
for (const k of ["APPLE_ID", "APPLE_PASSWORD", "APPLE_TEAM_ID", "APPLE_API_ISSUER", "APPLE_API_KEY", "APPLE_API_KEY_PATH"]) {
  delete env[k];
}
const p = Bun.spawnSync(["bun", "run", "tauri", "build", "--bundles", "app", "--config", conf], {
  cwd: root,
  stdout: "inherit",
  stderr: "inherit",
  env,
});
if (p.exitCode !== 0) process.exit(p.exitCode ?? 1);
console.log(`built ${join(root, "src-tauri/target/release/bundle/macos/Co-pet.app")}`);
