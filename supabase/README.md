# Deploying the Co-pet backend

1. `supabase link --project-ref <ref>`
2. `supabase db push`
3. `supabase secrets set NOTION_CLIENT_ID=… NOTION_CLIENT_SECRET=… ASANA_CLIENT_ID=… ASANA_CLIENT_SECRET=… COPET_SERVICE_KEY=<secret key from Project Settings › API keys>`
4. `supabase functions deploy oauth --no-verify-jwt`
5. Set the Notion public integration redirect URI and the Asana app redirect URL to `https://<ref>.supabase.co/functions/v1/oauth/callback`.
6. Register these scopes on the Asana app: `tasks:write workspaces:read projects:read attachments:write users:read`.
7. Build the app with `COPET_SUPABASE_URL=https://<ref>.supabase.co` and `COPET_SUPABASE_KEY=<publishable key>`.
