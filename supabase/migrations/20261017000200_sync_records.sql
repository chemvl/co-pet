create sequence public.records_seq;
create table public.records (
  user_id uuid not null references auth.users(id) on delete cascade,
  tbl text not null, id text not null,
  updated_at bigint not null, device_id text not null,
  deleted boolean not null default false,
  payload text not null,
  seq bigint not null,
  primary key (user_id, tbl, id));
create index records_pull on public.records (user_id, seq);
alter table public.records enable row level security;
create policy records_read on public.records for select using (user_id = auth.uid());
-- Clients only read directly; writes go through sync_push (security definer, runs as owner).
grant select on public.records to authenticated;

-- Writes only through this function. The per-user advisory lock makes each user's seq order equal commit
-- order, so a reader paging by seq never skips a row that commits late.
create function public.sync_push(rows jsonb) returns integer
language plpgsql security definer set search_path = public as $$
declare r jsonb; uid uuid := auth.uid(); n integer := 0; k integer;
begin
  if uid is null then raise exception 'not signed in'; end if;
  perform pg_advisory_xact_lock(hashtextextended(uid::text, 0));
  for r in select * from jsonb_array_elements(rows) loop
    insert into records as t (user_id, tbl, id, updated_at, device_id, deleted, payload, seq)
    values (uid, r->>'tbl', r->>'id', (r->>'updatedAt')::bigint, r->>'deviceId',
            coalesce((r->>'deleted')::boolean, false), r->>'payload', nextval('records_seq'))
    on conflict (user_id, tbl, id) do update
      set updated_at = excluded.updated_at, device_id = excluded.device_id, deleted = excluded.deleted,
          payload = excluded.payload, seq = nextval('records_seq')
      where (excluded.updated_at, excluded.device_id) > (t.updated_at, t.device_id);
    get diagnostics k = row_count; n := n + k;
  end loop;
  return n;
end $$;
revoke all on function public.sync_push(jsonb) from public, anon;
grant execute on function public.sync_push(jsonb) to authenticated;

insert into storage.buckets (id, name, public, file_size_limit)
values ('blobs', 'blobs', false, 52428800) on conflict (id) do nothing;
create policy blobs_own on storage.objects for all to authenticated
  using (bucket_id = 'blobs' and (storage.foldername(name))[1] = auth.uid()::text)
  with check (bucket_id = 'blobs' and (storage.foldername(name))[1] = auth.uid()::text);
