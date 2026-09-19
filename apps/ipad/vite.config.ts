import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// During development the app runs on Vite and talks to a local agent started with
// `vp-agentd --dev-loopback` (plain HTTP bound to 127.0.0.1 only).
const agent = process.env.VP_AGENT ?? 'http://127.0.0.1:47802';

export default defineConfig({
  plugins: [react()],
  base: '/',
  build: {
    target: ['safari16', 'chrome110'],
    sourcemap: false,
    assetsInlineLimit: 0,
    cssCodeSplit: false,
  },
  server: {
    proxy: {
      '/api': { target: agent, changeOrigin: false },
      '/ws': { target: agent.replace(/^http/, 'ws'), ws: true, changeOrigin: false },
    },
  },
});
