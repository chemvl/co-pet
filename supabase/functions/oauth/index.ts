// OAuth hand-off for Notion and Asana. The app opens /start in the browser with a random
// `state` and a PKCE-style `challenge`; the provider redirects to /callback, which exchanges the
// code with the client secret and parks the normalized token; the app then claims it with the
// verifier via /claim. /refresh runs the refresh grant with the secret held here.
import { createClient } from "npm:@supabase/supabase-js@2";
import {
  authorizeUrl,
  challengeFor,
  doneUrl,
  isB64url43,
  isProvider,
  normalizeToken,
  type Provider,
} from "./lib.ts";

const SUPABASE_URL = Deno.env.get("SUPABASE_URL") ?? "";
const REDIRECT_URI = `${SUPABASE_URL}/functions/v1/oauth/callback`;
const HANDOFF_TTL_MS = 10 * 60 * 1000;
const NOTION_TOKEN_URL = "https://api.notion.com/v1/oauth/token";
const ASANA_TOKEN_URL = "https://app.asana.com/-/oauth_token";

const db = createClient(SUPABASE_URL, Deno.env.get("COPET_SERVICE_KEY") ?? "", {
  auth: { persistSession: false, autoRefreshToken: false },
});

const clientId = (p: Provider) =>
  Deno.env.get(p === "notion" ? "NOTION_CLIENT_ID" : "ASANA_CLIENT_ID") ?? "";
const clientSecret = (p: Provider) =>
  Deno.env.get(p === "notion" ? "NOTION_CLIENT_SECRET" : "ASANA_CLIENT_SECRET") ?? "";

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function redirect(location: string): Response {
  return new Response(null, { status: 302, headers: { Location: location } });
}

/** Token endpoint call for both grants; `grant` holds grant_type plus code/redirect_uri or refresh_token. */
function tokenRequest(provider: Provider, grant: Record<string, string>): Promise<Response> {
  if (provider === "notion") {
    return fetch(NOTION_TOKEN_URL, {
      method: "POST",
      headers: {
        Authorization: `Basic ${btoa(`${clientId(provider)}:${clientSecret(provider)}`)}`,
        "Notion-Version": "2026-03-11",
        "Content-Type": "application/json",
      },
      body: JSON.stringify(grant),
    });
  }
  return fetch(ASANA_TOKEN_URL, {
    method: "POST",
    body: new URLSearchParams({
      ...grant,
      client_id: clientId(provider),
      client_secret: clientSecret(provider),
    }),
  });
}

async function readJson(req: Request): Promise<Record<string, unknown> | null> {
  try {
    const body: unknown = await req.json();
    return body && typeof body === "object" ? (body as Record<string, unknown>) : null;
  } catch {
    return null;
  }
}

async function start(url: URL): Promise<Response> {
  const provider = url.searchParams.get("provider");
  const state = url.searchParams.get("state");
  const challenge = url.searchParams.get("challenge");
  if (!isProvider(provider) || !isB64url43(state) || !isB64url43(challenge)) {
    return json(400, { error: "invalid" });
  }
  const cutoff = new Date(Date.now() - HANDOFF_TTL_MS).toISOString();
  await db.from("oauth_handoffs").delete().lt("created_at", cutoff);
  const { error } = await db.from("oauth_handoffs").insert({ state, provider, challenge });
  if (error) return json(500, { error: "server" });
  return redirect(authorizeUrl(provider, clientId(provider), REDIRECT_URI, state));
}

async function callback(url: URL): Promise<Response> {
  const state = url.searchParams.get("state") ?? "";
  const cutoff = new Date(Date.now() - HANDOFF_TTL_MS).toISOString();
  const { data: row } = isB64url43(state)
    ? await db.from("oauth_handoffs").select("provider").eq("state", state)
      .gte("created_at", cutoff).maybeSingle()
    : { data: null };
  if (!row || !isProvider(row.provider)) return redirect(doneUrl({ state, error: "expired" }));
  if (url.searchParams.get("error")) return redirect(doneUrl({ state, error: "denied" }));

  const provider = row.provider;
  const code = url.searchParams.get("code");
  const failed = redirect(doneUrl({ state, error: "exchange" }));
  if (!code) return failed;
  try {
    const res = await tokenRequest(provider, {
      grant_type: "authorization_code",
      code,
      redirect_uri: REDIRECT_URI,
    });
    if (!res.ok) return failed;
    const token = normalizeToken(provider, await res.json(), Date.now());
    if (!token) return failed;
    const { error } = await db.from("oauth_handoffs").update({ token_json: token }).eq("state", state);
    if (error) return failed;
  } catch {
    return failed;
  }
  return redirect(doneUrl({ provider, state }));
}

async function claim(req: Request): Promise<Response> {
  const body = await readJson(req);
  const invalid = json(400, { error: "invalid" });
  if (!body || !isB64url43(body.state) || typeof body.verifier !== "string" || !body.verifier) {
    return invalid;
  }
  // One statement matches state + challenge + parked token and deletes the row, so a token is
  // handed out at most once.
  const { data } = await db.from("oauth_handoffs").delete()
    .eq("state", body.state)
    .eq("challenge", await challengeFor(body.verifier))
    .not("token_json", "is", null)
    .select("token_json")
    .maybeSingle();
  return data ? json(200, data.token_json) : invalid;
}

async function refresh(req: Request): Promise<Response> {
  const body = await readJson(req);
  if (!body || !isProvider(body.provider) || typeof body.refreshToken !== "string" || !body.refreshToken) {
    return json(400, { error: "invalid" });
  }
  try {
    const res = await tokenRequest(body.provider, {
      grant_type: "refresh_token",
      refresh_token: body.refreshToken,
    });
    if (res.status >= 400 && res.status < 500) return json(401, { error: "unauthorized" });
    if (!res.ok) return json(502, { error: "exchange" });
    const token = normalizeToken(body.provider, await res.json(), Date.now(), false);
    return token ? json(200, token) : json(502, { error: "exchange" });
  } catch {
    return json(502, { error: "exchange" });
  }
}

Deno.serve((req) => {
  const url = new URL(req.url);
  const path = url.pathname;
  if (req.method === "GET" && path.endsWith("/oauth/start")) return start(url);
  if (req.method === "GET" && path.endsWith("/oauth/callback")) return callback(url);
  if (req.method === "POST" && path.endsWith("/oauth/claim")) return claim(req);
  if (req.method === "POST" && path.endsWith("/oauth/refresh")) return refresh(req);
  return json(404, { error: "notFound" });
});
