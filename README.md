# Co-pet

Co-pet is a desktop companion that lives on the edge of your screen as an animated pet and works as a universal inbox. Drop anything on it — a file, a link, a screenshot, a snippet of text — and it sorts it, turns the actionable parts into tasks, tells you the one thing to do next, and hands content back when you need it: drag it out into another app, export it to Notion or Asana, or let your AI agents read it over MCP.

It is local-first: items, tasks and the triage model live on your Mac. Nothing leaves the machine unless you turn on a feature that needs the network, and synced data is end-to-end encrypted.

Platform: macOS 13+ on Apple Silicon. UI in English and Vietnamese.

## What it does

- **Capture** — drop onto the pet, paste, quick note, screen capture, or "send selection" from any app (global hotkeys, default `⌘⇧Space` focus pet, `⌘⇧C` send selection, `⌘⇧S` capture screen, `⌘⇧N` quick note).
- **Triage** — rules first, then a small local model (llama.cpp `llama-server`) files each item into a category. Every decision can be undone. Items that look like secrets are marked sensitive and never sent to the model or to agents.
- **Home** — Inbox, item detail, Tasks, Categories & rules editor, Integrations, Agents, Settings, and a command palette (`⌘K`).
- **Tasks and What's Next** — convert items to tasks; the pet suggests the next task when you come back to your Mac or finish something.
- **Export** — push tasks to Notion or Asana through a retrying queue.
- **MCP server** — AI agents (Claude Code, Cursor, Codex, Gemini CLI, or any MCP client) can search items and manage tasks through the bundled `copet-mcp` sidecar, with per-client write approval and an undoable activity log.
- **Accounts and sync** — optional sign-in (email code or Google) and end-to-end encrypted sync between your Macs through Supabase.

## Repository layout

| Path | What |
| --- | --- |
| `src-tauri/` | Rust core (Tauri 2): database, capture, triage, tasks, export, MCP socket API, accounts, sync |
| `src/` | React UI — `pet/` (the pet window), `home/` (Home window), `shared/` (IPC, types, i18n), `ds/` (design system) |
| `src-mcp/` | `copet-mcp` sidecar (Bun + MCP SDK), compiled into `src-tauri/binaries/` |
| `supabase/` | Backend for OAuth hand-off, account deletion and sync (migrations + Edge Functions) |
| `scripts/` | `fetch-llama.ts`, `build-mcp.ts`, `render-icons.ts` |
| `docs/` | Product brief, UI & UX guidelines, design boards, plans and handoff notes |

## Run it locally

### 1. Prerequisites

- macOS 13 or later on Apple Silicon, with Xcode Command Line Tools (`xcode-select --install`).
- [Rust](https://rustup.rs) (stable).
- [Bun](https://bun.sh) 1.4 or later.
- Optional, only for accounts/sync/OAuth: the [Supabase CLI](https://supabase.com/docs/guides/cli) and Docker (Docker Desktop or colima).

### 2. Install and fetch the local model runtime

```sh
bun install
bun run fetch:llama   # downloads the pinned llama.cpp build into src-tauri/resources/llama/ (once)
```

### 3. Start the app in development mode

```sh
bun run tauri dev
```

This builds the `copet-mcp` sidecar, starts Vite, compiles the Rust app and opens it. The first run shows onboarding, which downloads the local triage model (sized for your Mac's memory). Until the model is ready, items are sorted by rules only and the rest land in Unsorted.

Notes:

- Dev mode uses the real app data folder `~/Library/Application Support/com.copet.app/` (database, library files, models). To start fresh use Settings › Data › Delete everything, or quit the app and remove that folder.
- macOS asks for **Screen Recording** (screen capture) and **Accessibility** (send selection, paste back) the first time you use those features. The dev binary and the bundled app get separate grants.
- Without a backend configured, the app runs fully local; Integrations shows "Use a token instead" for Notion/Asana and Settings › Account says sync isn't set up in this build.

### 4. Build the app bundle

```sh
bun run tauri build --bundles app
open src-tauri/target/release/bundle/macos/Co-pet.app
```

Launch the bundle with `open`, not from a terminal — otherwise macOS attributes permissions to the terminal. `copet://` deep links (Notion/Asana OAuth, Google sign-in, item links) only work in the bundled app.

### 5. Run the checks

```sh
bun run build:mcp                      # required before cargo: tauri-build needs the sidecar binary
(cd src-tauri && cargo test && cargo check)
bun run test
bun run typecheck
```

## Optional: run with a local backend

The backend URL and key are baked in at compile time from `COPET_SUPABASE_URL` and `COPET_SUPABASE_KEY`. To try accounts and sync against a local Supabase:

```sh
supabase start        # applies supabase/migrations; prints the API URL and keys
supabase status       # API URL (default http://127.0.0.1:54321), publishable/anon key, service_role key
```

Serve the Edge Functions with an env file (dummy Notion/Asana values are fine unless you test OAuth):

```sh
cat > /tmp/copet-functions.env <<'EOF'
COPET_SERVICE_KEY=<service_role key from supabase status>
NOTION_CLIENT_ID=dummy
NOTION_CLIENT_SECRET=dummy
ASANA_CLIENT_ID=dummy
ASANA_CLIENT_SECRET=dummy
EOF
supabase functions serve --env-file /tmp/copet-functions.env
```

Then, in another terminal, start the app with the backend configured:

```sh
export COPET_SUPABASE_URL=http://127.0.0.1:54321
export COPET_SUPABASE_KEY=<publishable/anon key from supabase status>
bun run tauri dev
```

Sign in from Settings › Account and sync with any email; the 6-digit code arrives in the local mail catcher at http://127.0.0.1:54324. Google sign-in and Notion/Asana OAuth need a hosted project and the bundled app — see [`supabase/README.md`](supabase/README.md) for deploying the hosted backend.

## Connecting an AI agent

Open Home › Agents and click **Add to Claude Code / Cursor / Codex / Gemini CLI**, or copy the stdio command shown there into any MCP client. Agents can read right away; writes need **Approve writes** for that client. While the app runs it also serves MCP over HTTP at `http://127.0.0.1:4823/mcp` for clients holding a token created on the Agents screen.

## Documentation

- [Product brief](docs/Desktop%20Pet%20Inbox%20%E2%80%94%20Product%20Brief.md)
- [UI & UX guidelines](docs/Co-pet%20%E2%80%94%20UI%20%26%20UX%20guidelines.md)
- [Phase 2 plan](docs/handoff/copet-phase2-plan.md) and session handoffs in `docs/handoff/`
