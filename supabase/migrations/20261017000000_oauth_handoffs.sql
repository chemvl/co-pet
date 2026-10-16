create table public.oauth_handoffs (
  state text primary key,
  provider text not null check (provider in ('notion','asana')),
  challenge text not null,
  token_json jsonb,
  created_at timestamptz not null default now());
alter table public.oauth_handoffs enable row level security;  -- no policies: only the service key can touch it
-- New projects no longer grant API roles table privileges by default; the function's service key needs them.
grant select, insert, update, delete on public.oauth_handoffs to service_role;
