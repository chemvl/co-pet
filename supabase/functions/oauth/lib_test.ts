import { assert, assertEquals } from "jsr:@std/assert@1";
import { authorizeUrl, base64url, challengeFor, doneUrl, isB64url43, normalizeToken } from "./lib.ts";

Deno.test("isB64url43 accepts only 43-char unpadded base64url", () => {
  const ok = "A".repeat(40) + "-_9";
  assert(isB64url43(ok));
  assert(!isB64url43(ok.slice(1)));
  assert(!isB64url43(ok + "A"));
  assert(!isB64url43("A".repeat(42) + "+"));
  assert(!isB64url43("A".repeat(42) + "="));
  assert(!isB64url43(null));
});

Deno.test("challengeFor is base64url(sha256(verifier)) per RFC 7636 appendix B", async () => {
  assertEquals(
    await challengeFor("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
    "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
  );
  assertEquals(base64url(new Uint8Array([251, 255])), "-_8");
});

Deno.test("authorizeUrl builds provider URLs with encoded redirect and scope", () => {
  const n = new URL(authorizeUrl("notion", "nid", "https://x.supabase.co/functions/v1/oauth/callback", "s"));
  assertEquals(n.origin + n.pathname, "https://api.notion.com/v1/oauth/authorize");
  assertEquals(n.searchParams.get("owner"), "user");
  assertEquals(n.searchParams.get("response_type"), "code");
  assertEquals(n.searchParams.get("redirect_uri"), "https://x.supabase.co/functions/v1/oauth/callback");
  const a = authorizeUrl("asana", "aid", "https://r", "s");
  assert(a.startsWith("https://app.asana.com/-/oauth_authorize?client_id=aid&response_type=code"));
  assert(a.endsWith("&scope=tasks%3Aread%20tasks%3Awrite%20workspaces%3Aread%20projects%3Aread%20attachments%3Awrite%20users%3Aread"));
});

Deno.test("doneUrl encodes params", () => {
  assertEquals(doneUrl({ state: "a b", error: "expired" }), "copet://oauth/done?state=a%20b&error=expired");
});

Deno.test("normalizeToken maps Notion, Asana and refresh responses", () => {
  assertEquals(
    normalizeToken("notion", { access_token: "t", refresh_token: "r", expires_in: 60, workspace_name: "W" }, 1000),
    { accessToken: "t", refreshToken: "r", expiresAt: 61000, accountLabel: "W" },
  );
  assertEquals(
    normalizeToken("asana", { access_token: "t", expires_in: 3600, data: { email: "a@b.c" } }, 0),
    { accessToken: "t", refreshToken: null, expiresAt: 3_600_000, accountLabel: "a@b.c" },
  );
  assertEquals(
    normalizeToken("notion", { access_token: "t", workspace_name: "W" }, 0, false),
    { accessToken: "t", refreshToken: null, expiresAt: null, accountLabel: "" },
  );
  assertEquals(normalizeToken("asana", { error: "invalid_grant" }, 0), null);
  assertEquals(normalizeToken("asana", null, 0), null);
});
