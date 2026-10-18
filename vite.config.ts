import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  // Rust builds write HTML under src-tauri/target; watching it reloads every window.
  server: { port: 1420, strictPort: true, watch: { ignored: ['**/src-tauri/**'] } },
  build: {
    // WebView2 (Chromium) on Windows, WKWebView on macOS; Tauri sets TAURI_ENV_PLATFORM.
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari16',
    rollupOptions: {
      input: { pet: 'pet.html', home: 'home.html', onboarding: 'onboarding.html' },
    },
  },
});
