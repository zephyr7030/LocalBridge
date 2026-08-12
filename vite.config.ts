import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  root: "src",
  plugins: [react()],
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
  },
  build: {
    outDir: "../tests/artifacts/frontend-dist",
    emptyOutDir: true,
  },
  test: {
    include: ["__tests__/**/*.test.ts"],
  },
});
