create table public.sync_keys (
  user_id uuid primary key default auth.uid() references auth.users(id) on delete cascade,
  wrapped_key text not null,                 -- base64(0x01 || nonce24 || XChaCha20-Poly1305(KEK, MK))
  created_at timestamptz not null default now());
alter table public.sync_keys enable row level security;
create policy sync_keys_read on public.sync_keys for select using (user_id = auth.uid());
create policy sync_keys_insert on public.sync_keys for insert with check (user_id = auth.uid());
-- New projects no longer grant API roles table privileges by default; signed-in clients need these.
grant select, insert on public.sync_keys to authenticated;
