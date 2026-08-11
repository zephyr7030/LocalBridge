import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  root: "src",
  plugins: [react()],
  build: {
    outDir: "../tests/artifacts/frontend-dist",
    emptyOutDir: true,
  },
  test: {
    include: ["__tests__/**/*.test.ts"],
  },
});
