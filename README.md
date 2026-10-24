# Co-pet

Co-pet is a desktop companion that lives on the edge of your screen as an animated pet and works as a universal inbox. Drop anything on it — a file, a link, a screenshot, a snippet of text — and it sorts it, turns the actionable parts into tasks, tells you the one thing to do next, and hands content back when you need it: drag it out into another app, export it to Notion or Asana, or let your AI agents read it over MCP.

It is local-first: items, tasks and the triage model live on your computer. Nothing leaves the machine unless you turn on a feature that needs the network, and synced data is end-to-end encrypted.

Distributed for Windows 11 x64 through the Microsoft Store, free. The macOS code (macOS 13+ on Apple Silicon) stays in the repo and builds for development, but isn't distributed. UI in English and Vietnamese.

## What it does

- **Capture** — drop onto the pet, paste, quick note, screen capture, or "send selection" from any app (global hotkeys, default `⌘/Ctrl+Shift+Space` focus pet, `⌘/Ctrl+Shift+C` send selection, `⌘/Ctrl+Shift+S` capture screen, `⌘/Ctrl+Shift+N` quick note).
- **Triage** — rules first, then a small local model (llama.cpp `llama-server`) files each item into a category. Every decision can be undone. Items that look like secrets are marked sensitive and never sent to the model or to agents.
- **Home** — Inbox, item detail, Tasks, Categories & rules editor, Integrations, Agents, Settings, and a command palette (`⌘/Ctrl+K`).
- **Tasks and What's Next** — convert items to tasks; the pet suggests the next task when you come back to your computer or finish something.
- **Export** — push tasks to Notion or Asana through a retrying queue.
- **Transcribe** — records meetings on this computer and turns them into a transcript, summary and tasks; offers to record Zoom and Google Meet calls (Settings › Transcribe).
- **MCP server** — AI agents (Claude Code, Cursor, Codex, Gemini CLI, or any MCP client) can search items and manage tasks through the bundled `copet-mcp` sidecar, with per-client write approval and an undoable activity log.
- **Accounts and sync** — optional sign-in (email code or Google) and end-to-end encrypted sync between your computers through a Supabase project you run yourself (see [Bring your own backend](#bring-your-own-backend)).

## Repository layout

| Path | What |
| --- | --- |
| `src-tauri/` | Rust core (Tauri 2): database, capture, triage, tasks, export, MCP socket API, accounts, sync |
| `src/` | React UI — `pet/` (the pet window), `home/` (Home window), `shared/` (IPC, types, i18n), `ds/` (design system) |
| `src-mcp/` | `copet-mcp` sidecar (Bun + MCP SDK), compiled into `src-tauri/binaries/` |
| `supabase/` | Backend for OAuth hand-off, account deletion and sync (migrations + Edge Functions), deployed by each user to their own Supabase project |
| `scripts/` | `fetch-llama.ts`, `build-mcp.ts`, `build-local.ts`, `package-msix.ts`, `set-version.ts`, `publish-site.ts`, `render-icons.ts` |
| `video/` | 60-second intro video (Remotion) built from the real design system and pet art — `cd video && bun run render` writes `out/co-pet-intro.mp4`; `bun run render:music` mixes in `public/music.mp3` |
| `docs/` | Product brief, UI & UX guidelines, design boards, plans and handoff notes |

## Run it locally

### 1. Prerequisites

- macOS 13 or later on Apple Silicon, with Xcode Command Line Tools (`xcode-select --install`).
- Windows 11 x64: Visual Studio 2022 Build Tools with "Desktop development with C++", [Rust](https://rustup.rs) stable (MSVC), [Bun](https://bun.sh) 1.4+, CMake 3.28+, Ninja, LLVM 17+ and the Vulkan SDK for whisper's GPU build (`winget install Kitware.CMake Ninja-build.Ninja LLVM.LLVM KhronosGroup.VulkanSDK`). Build Windows on Windows; cross-compiling from macOS isn't supported.
  On a Windows machine without a GPU driver that provides `vulkan-1.dll` (for example a VM), the test binaries need the Vulkan Runtime's `vulkan-1.dll` on `PATH` to start (CI adds the runtime its Vulkan SDK action installs).
  Run every Windows build (`cargo`, `tauri dev`, `tauri build`, `package:msix`) from an "x64 Native Tools Command Prompt for VS 2022" (or after `call "<VS>\VC\Auxiliary\Build\vcvars64.bat"`), with Ninja, CMake and `C:\Program Files\LLVM\bin` on `PATH` and these variables set (Ninja because with the Visual Studio generator, whisper.cpp's Vulkan shader build exceeds the Windows path length limit):

  ```bat
  set CMAKE_GENERATOR=Ninja
  set VULKAN_SDK=C:\VulkanSDK\<version>
  set LIBCLANG_PATH=C:\Program Files\LLVM\bin
  ```
- Windows only, for MSIX packages: the Windows 10/11 SDK (`makeappx.exe`, `makepri.exe`, `signtool.exe`).
- [Rust](https://rustup.rs) (stable).
- [Bun](https://bun.sh) 1.4 or later.
- Optional, only for accounts/sync/OAuth: the [Supabase CLI](https://supabase.com/docs/guides/cli) and Docker (Docker Desktop or colima).

### 2. Install and fetch the local model runtime

```sh
bun install
bun run fetch:llama   # downloads the pinned llama.cpp build for this OS (macOS arm64, Windows x64 Vulkan) into src-tauri/resources/llama/ (once per OS)
```

### 3. Start the app in development mode

```sh
bun run tauri dev
```

This builds the `copet-mcp` sidecar, starts Vite, compiles the Rust app and opens it. The first run shows onboarding, which downloads the local triage model (sized for your computer's memory). Until the model is ready, items are sorted by rules only and the rest land in Unsorted.

Notes:

- On macOS, dev mode uses the real app data folder `~/Library/Application Support/com.copet.app/` (database, library files, models). To start fresh use Settings › Data › Delete everything, or quit the app and remove that folder.
- macOS asks for **Screen Recording** (screen capture) and **Accessibility** (send selection, paste back) the first time you use those features. The dev binary and the bundled app get separate grants.
- Until you connect a backend in Settings › Account and sync, the app runs fully local; Integrations shows "Use a token instead" for Notion/Asana. See [Bring your own backend](#bring-your-own-backend).

Windows notes:

- In `tauri dev` the app data folder is `%APPDATA%\com.copet.app\` (database, library files, models). The packaged (MSIX) app uses `%USERPROFILE%\Co-pet\` instead.
- `copet://` links work in `tauri dev` too: the dev app registers the scheme at runtime.
- Windows has no Screen Recording or Accessibility prompts.
- Calendar isn't available on Windows, so What's Next doesn't use calendar events.
- To build and install a sideloadable MSIX test package, see [4. Build the app](#4-build-the-app).

### 4. Build the app

macOS (development only, not distributed):

```sh
bun run build:local
open src-tauri/target/release/bundle/macos/Co-pet.app
```

Launch the bundle with `open`, not from a terminal — otherwise macOS attributes permissions to the terminal. `copet://` deep links (Notion/Asana OAuth, Google sign-in, item links) only work in the bundled app.

Windows (signed with a local self-signed certificate, for sideload testing):

```bat
bun run package:msix --dev-cert
```

It builds the app and writes `src-tauri\target\release\msix\Co-pet_<x.y.z.0>_x64.msix`. Trust the certificate once with the admin PowerShell command it prints, then install with `Add-AppxPackage "<path-to-msix>"`. The packaged app keeps its data in `%USERPROFILE%\Co-pet\`.

Step-by-step local builds and Microsoft Store releases: [Build and release](docs/Co-pet%20%E2%80%94%20Build%20and%20release.md).

### 5. Run the checks

```sh
bun run build:mcp                      # required before cargo: tauri-build needs the sidecar binary
(cd src-tauri && cargo test && cargo check)
bun run test
bun run typecheck
```

## Bring your own backend

Builds contain no backend. Accounts, sync, Google sign-in and Notion/Asana OAuth go through a Supabase project that each user deploys and connects themselves: paste its URL and **publishable** key in Settings › Account and sync. Co-pet saves them on this device only (never synced) and runs fully local until then. Never paste the service_role/secret key into the app. Notion and Asana personal tokens work without any backend.

To deploy the backend to a hosted project (migrations, the `oauth` and `delete-account` functions, their secrets, your own Notion/Asana apps with redirect URL `https://<ref>.supabase.co/functions/v1/oauth/callback`, the `copet://auth/callback` redirect and the optional Google provider), follow [`supabase/README.md`](supabase/README.md). Function secrets go in `supabase secrets set` and stay in Supabase; none of them are part of the app.

### Local backend

```sh
supabase start        # applies supabase/migrations; prints the API URL and keys
supabase status       # API URL (default http://127.0.0.1:54321), publishable key, secret (service_role) key
```

Serve the Edge Functions. Dummy Notion/Asana values are fine unless you test OAuth:

```sh
cp supabase/functions/.env.example supabase/functions/.env.local   # then fill in COPET_SERVICE_KEY
supabase functions serve --env-file supabase/functions/.env.local
```

Start the app with `bun run tauri dev`, open Settings › Account and sync, and paste `http://127.0.0.1:54321` and the local publishable key. Sign in with any email; the 6-digit code arrives in the local mail catcher at http://127.0.0.1:54324.

Google sign-in and Notion/Asana OAuth return through `copet://` deep links. macOS delivers those only to the bundled app, so on macOS these flows can't finish in `tauri dev`. Use a hosted project to test them: `supabase/config.toml` has no Google provider and doesn't allow `copet://auth/callback`, and the local `oauth` function's redirect URI isn't reachable from Notion or Asana.

On macOS the keychain trusts an app by its code signature. `build:local` signs with the first "Developer ID Application" or "Apple Development" certificate in your login keychain (or `APPLE_SIGNING_IDENTITY`), so after you click **Always Allow** once per Co-pet keychain entry, rebuilt apps read it without asking. Ad-hoc builds (`signingIdentity: "-"`, used when no certificate exists) are a new app to the keychain on every build and ask for the login password again; within one launch the app reads each entry at most once.

## Connecting an AI agent

Open Home › Agents and click **Add to Claude Code / Cursor / Codex / Gemini CLI**, or copy the stdio command shown there into any MCP client. Agents can read right away; writes need **Approve writes** for that client. While the app runs it also serves MCP over HTTP at `http://127.0.0.1:4823/mcp` for clients holding a token created on the Agents screen.

## Documentation

- [Product brief](docs/Desktop%20Pet%20Inbox%20%E2%80%94%20Product%20Brief.md)
- [UI & UX guidelines](docs/Co-pet%20%E2%80%94%20UI%20%26%20UX%20guidelines.md)
- [Build and release](docs/Co-pet%20%E2%80%94%20Build%20and%20release.md)
- [Phase 2 plan](docs/handoff/copet-phase2-plan.md) and session handoffs in `docs/handoff/`
