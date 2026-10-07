/// <reference types="vitest/config" />
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  // Keep Rust errors visible in `tauri dev`.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  // Pre-bundling rewrites @wterm/ghostty into .vite/deps, which breaks its
  // `new URL("../wasm/ghostty-vt.wasm", import.meta.url)` lookup.
  optimizeDeps: {
    exclude: ["@wterm/core", "@wterm/dom", "@wterm/ghostty", "@wterm/react"],
  },
  build: { target: "safari18", sourcemap: true },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["src/test/setup.ts"],
    css: { modules: { classNameStrategy: "non-scoped" } },
  },
});
