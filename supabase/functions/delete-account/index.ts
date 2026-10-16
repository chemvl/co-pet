// Deletes the caller's account: their encrypted blobs under `blobs/<uid>/`, then the auth user
// (which cascades `sync_keys` and, from M9, `records`). The gateway verifies the JWT.
import { createClient } from "npm:@supabase/supabase-js@2";

const PAGE = 1000;

const admin = createClient(
  Deno.env.get("SUPABASE_URL") ?? "",
  Deno.env.get("COPET_SERVICE_KEY") ?? "",
  { auth: { persistSession: false, autoRefreshToken: false } },
);

const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });

Deno.serve(async (req) => {
  if (req.method !== "POST") return json(405, { error: "method_not_allowed" });
  const jwt = (req.headers.get("Authorization") ?? "").replace(/^Bearer\s+/i, "");
  const { data: { user }, error: userErr } = await admin.auth.getUser(jwt);
  if (userErr || !user) return json(401, { error: "unauthorized" });
  const uid = user.id;

  const bucket = admin.storage.from("blobs");
  // Removing objects shifts later pages forward, so always list from offset 0 until empty.
  for (;;) {
    const { data: objects, error } = await bucket.list(uid, { limit: PAGE, offset: 0 });
    if (error) return json(500, { error: "storage_list_failed" });
    if (!objects || objects.length === 0) break;
    const { data: removed, error: rmErr } = await bucket.remove(
      objects.map((o) => `${uid}/${o.name}`),
    );
    // Blobs are flat under `<uid>/`; an empty result on a non-empty page (e.g. a folder prefix entry)
    // would otherwise list the same page forever.
    if (rmErr || !removed || removed.length === 0) {
      return json(500, { error: "storage_remove_failed" });
    }
    if (objects.length < PAGE) break;
  }

  const { error: delErr } = await admin.auth.admin.deleteUser(uid);
  if (delErr) return json(500, { error: "delete_user_failed" });
  return json(200, {});
});
