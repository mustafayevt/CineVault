import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// The dev server port is fixed because src-tauri/tauri.conf.json points at it.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
    // CHANGELOG.md at the repository root is bundled into the app (`?raw` import).
    fs: { allow: [".."] },
  },
  build: {
    target: "es2022",
    sourcemap: true,
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    css: false,
    // Guards against a hang, not a speed budget: a loaded machine must not fail a slow test.
    testTimeout: 30_000,
    hookTimeout: 30_000,
  },
});
