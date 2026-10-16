# Deploying the Co-pet backend

1. `supabase link --project-ref <ref>`
2. `supabase db push`
3. `supabase secrets set NOTION_CLIENT_ID=… NOTION_CLIENT_SECRET=… ASANA_CLIENT_ID=… ASANA_CLIENT_SECRET=… COPET_SERVICE_KEY=<secret key from Project Settings › API keys>`
4. `supabase functions deploy oauth --no-verify-jwt`
5. Set the Notion public integration redirect URI and the Asana app redirect URL to `https://<ref>.supabase.co/functions/v1/oauth/callback`.
6. Register these scopes on the Asana app: `tasks:write workspaces:read projects:read attachments:write users:read`.
7. Build the app with `COPET_SUPABASE_URL=https://<ref>.supabase.co` and `COPET_SUPABASE_KEY=<publishable key>`.
8. `supabase functions deploy delete-account` (JWT verified by the gateway).
9. Dashboard › Authentication › Providers › Email: enabled, "Confirm email" on, email OTP length 6, OTP expiry 600 s.
10. Dashboard › Authentication › Email templates › "Magic Link": the body contains `{{ .Token }}` (e.g. "Your Co-pet sign-in code is {{ .Token }}") so the email carries a code.
11. Dashboard › Authentication › Providers › Google: enabled with a Google Cloud OAuth client (type Web application, authorized redirect URI `https://<ref>.supabase.co/auth/v1/callback`).
12. Dashboard › Authentication › URL Configuration › Redirect URLs: add `copet://auth/callback`.
