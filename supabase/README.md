# Running your own Co-pet backend

Co-pet works fully on your computer without any backend. Accounts, sync between your computers and the one-click "Connect Notion / Asana" buttons need a Supabase project that **you** own. Co-pet has no shared server: you create a free Supabase project, deploy the pieces in this folder to it, and paste its address into Co-pet.

Notion and Asana personal tokens ("Use a token instead" in Integrations) never need a backend.

## What each feature needs

| Feature | Needs |
| --- | --- |
| Sign in with an emailed code | Steps 1–3, 4 (Email), 5 |
| Sign in with Google | Steps 1–3, 4 (Google), 5 |
| Sync between computers (end-to-end encrypted) | A signed-in account; the database from step 2 |
| Delete cloud data | The `delete-account` function and `COPET_SERVICE_KEY` (step 3) |
| Connect Notion / Asana with OAuth | The `oauth` function, its secrets (step 3) and your own Notion / Asana app (step 6) |

## 1. Create the project

1. Sign up at https://supabase.com and create a project. Note its **Project ref** (the `<ref>` in `https://<ref>.supabase.co`).
2. Install the Supabase CLI (https://supabase.com/docs/guides/cli) and get this repository (`git clone https://github.com/chemvl/co-pet`, or download it as a ZIP).
3. In the repository folder:

   ```sh
   supabase login
   supabase link --project-ref <ref>
   ```

## 2. Create the database

```sh
supabase db push
```

This creates the encrypted sync tables (`records`, `sync_keys`), the private Storage bucket `blobs` for synced files, and the table the OAuth hand-off uses. Row-level security limits every user to their own rows and files. The server only ever sees encrypted data.

## 3. Deploy the functions and their secrets

1. In Dashboard › Project Settings › API keys, copy the **secret** key (`sb_secret_…`, or the legacy `service_role` key). It stays on the server: never paste it into Co-pet.
2. Set the secrets. Leave out the Notion or Asana pair if you don't use that OAuth connection:

   ```sh
   supabase secrets set \
     COPET_SERVICE_KEY=<secret key> \
     COPET_OAUTH_DONE_URL=https://mrdeeds88.github.io/oauth/done/ \
     NOTION_CLIENT_ID=<from step 6> NOTION_CLIENT_SECRET=<from step 6> \
     ASANA_CLIENT_ID=<from step 6> ASANA_CLIENT_SECRET=<from step 6>
   ```

   `COPET_OAUTH_DONE_URL` is a static page that hands the finished sign-in back to the app (it opens `copet://oauth/done`). It holds no data; you may host your own copy of it.
3. Deploy:

   ```sh
   supabase functions deploy oauth --no-verify-jwt
   supabase functions deploy delete-account
   ```

   `delete-account` keeps JWT verification on: only a signed-in user can delete their own account.

## 4. Turn on sign-in

**Email code**

1. Dashboard › Authentication › Providers › Email: enabled, "Confirm email" on, email OTP length `6`, OTP expiry `600` seconds.
2. Dashboard › Authentication › Emails › SMTP Settings: add a custom SMTP server (for example Resend: host `smtp.resend.com`, port `465`, user `resend`, password your Resend API key). The built-in mailer sends only 2 emails an hour and doesn't let you change the email templates, so codes can't be sent without your own SMTP server. Then raise Authentication › Rate Limits › emails per hour (for example `30`).
3. Dashboard › Authentication › Email templates › "Magic Link" and "Confirm signup": subject "Your Co-pet sign-in code", body from `templates/sign_in_code.html` (it contains `{{ .Token }}`). A new user gets "Confirm signup", an existing one "Magic Link".

**Google (optional)**

1. In Google Cloud Console › Google Auth Platform, create an OAuth client of type **Web application** with the authorized redirect URI `https://<ref>.supabase.co/auth/v1/callback`. While the app is in Testing, add your own Google accounts as test users.
2. Dashboard › Authentication › Providers › Google: enable it and paste the client ID and secret.

## 5. Let sign-in come back to Co-pet

Dashboard › Authentication › URL Configuration › Redirect URLs: add `copet://auth/callback`. Without it, Supabase ignores the app's return address and sends the browser to the Site URL instead.

## 6. Your own Notion and Asana apps (OAuth, optional)

The redirect address for both is `https://<ref>.supabase.co/functions/v1/oauth/callback`.

**Notion.** At https://www.notion.so/profile/integrations create a **public** integration (connection). Set its redirect URI to the address above, and under Capabilities enable Read content, Update content and Insert content. Use its OAuth client ID and secret for `NOTION_CLIENT_ID` / `NOTION_CLIENT_SECRET`. Choose the installation scope that covers the workspaces you want to use; it can't be changed later.

**Asana.** At https://app.asana.com/0/my-apps create an app. Add the redirect URL above under OAuth, and register exactly these scopes: `tasks:read tasks:write workspaces:read projects:read attachments:write users:read`. Use its client ID and secret for `ASANA_CLIENT_ID` / `ASANA_CLIENT_SECRET`.

After changing the secrets, run `supabase secrets set …` again; the functions pick them up without a redeploy.

## 7. Connect Co-pet

1. Dashboard › Project Settings › API keys: copy the **publishable** key (`sb_publishable_…`, or the legacy `anon` key). It's meant for apps; row-level security protects your data.
2. In Co-pet, open Settings › Account and sync, paste `https://<ref>.supabase.co` and the publishable key, and click **Test and save**. Co-pet checks that the project answers before saving it.

Sign-in, sync and the OAuth buttons work right away, without a restart. **Disconnect** in the same place signs you out and forgets the project; everything on your computer stays. Moving to another project signs you out first, and Notion or Asana connected through the old project's OAuth app have to be connected again (connections made with a token stay).

## Developing against a local stack

`supabase start` runs everything locally. Copy `functions/.env.example` to the gitignored `functions/.env.local`, then `supabase functions serve --env-file supabase/functions/.env.local`. In Co-pet, enter `http://127.0.0.1:54321` and the local publishable key from `supabase status` (plain `http://` is accepted only for `localhost` and `127.0.0.1`).
