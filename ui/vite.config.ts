import { defineConfig } from 'vitest/config'

// Tauri expects a fixed dev port and must see its own CLI output.
export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    // macOS WKWebView in Tauri 2 targets Safari 15+.
    target: 'safari15',
    minify: true,
    sourcemap: false,
  },
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
})
