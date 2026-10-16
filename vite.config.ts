import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: 'safari16',
    rollupOptions: {
      input: { pet: 'pet.html', home: 'home.html', onboarding: 'onboarding.html' },
    },
  },
});
