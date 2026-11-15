-- Google Tasks joins Notion and Asana in the OAuth hand-off. Replaces an earlier check that named
-- it `googleTasks`; handoffs live 10 minutes, so ones with a provider the app no longer sends go.
alter table public.oauth_handoffs drop constraint if exists oauth_handoffs_provider_check;
delete from public.oauth_handoffs where provider not in ('notion','asana','google_tasks');
alter table public.oauth_handoffs add constraint oauth_handoffs_provider_check
  check (provider in ('notion','asana','google_tasks'));
