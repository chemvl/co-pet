// Pure helpers for the OAuth hand-off function (no I/O, testable with `deno test`).

export type Provider = "notion" | "asana";

export interface TokenJson {
  accessToken: string;
  refreshToken: string | null;
  expiresAt: number | null;
  accountLabel: string;
}

export const ASANA_SCOPES =
  "tasks:read tasks:write workspaces:read projects:read attachments:write users:read";

export function isProvider(v: unknown): v is Provider {
  return v === "notion" || v === "asana";
}

/** `state` and `challenge` are 32 random bytes in unpadded base64url: exactly 43 chars. */
export function isB64url43(v: unknown): v is string {
  return typeof v === "string" && /^[A-Za-z0-9_-]{43}$/.test(v);
}

export function base64url(bytes: Uint8Array): string {
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** PKCE S256: base64url(sha256(verifier)). */
export async function challengeFor(verifier: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier));
  return base64url(new Uint8Array(digest));
}

function q(params: [string, string][]): string {
  return params.map(([k, v]) => `${k}=${encodeURIComponent(v)}`).join("&");
}

export function authorizeUrl(
  provider: Provider,
  clientId: string,
  redirectUri: string,
  state: string,
): string {
  if (provider === "notion") {
    return "https://api.notion.com/v1/oauth/authorize?" + q([
      ["client_id", clientId],
      ["response_type", "code"],
      ["owner", "user"],
      ["redirect_uri", redirectUri],
      ["state", state],
    ]);
  }
  return "https://app.asana.com/-/oauth_authorize?" + q([
    ["client_id", clientId],
    ["response_type", "code"],
    ["redirect_uri", redirectUri],
    ["state", state],
    ["scope", ASANA_SCOPES],
  ]);
}

export function doneUrl(params: Record<string, string>): string {
  return "copet://oauth/done?" + q(Object.entries(params));
}

/**
 * Normalizes a provider token response. Notion labels the account with `workspace_name`,
 * Asana with `data.email`; refresh responses pass `withLabel = false` (the app keeps its label).
 * Returns null when the response carries no access token.
 */
export function normalizeToken(
  provider: Provider,
  body: unknown,
  nowMs: number,
  withLabel = true,
): TokenJson | null {
  if (!body || typeof body !== "object") return null;
  const r = body as Record<string, unknown>; // object checked above; every field is type-checked below
  if (typeof r.access_token !== "string" || r.access_token === "") return null;
  let accountLabel = "";
  if (withLabel) {
    const data = r.data && typeof r.data === "object" ? (r.data as Record<string, unknown>) : {};
    const label = provider === "notion" ? r.workspace_name : data.email;
    accountLabel = typeof label === "string" ? label : "";
  }
  return {
    accessToken: r.access_token,
    refreshToken: typeof r.refresh_token === "string" ? r.refresh_token : null,
    expiresAt: typeof r.expires_in === "number" ? nowMs + r.expires_in * 1000 : null,
    accountLabel,
  };
}
