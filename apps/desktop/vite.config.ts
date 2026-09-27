import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 5174, strictPort: true },
  // Tauri renders with WKWebView (Safari 16+ on macOS 13).
  build: { target: 'safari16', sourcemap: false, cssCodeSplit: false },
});
