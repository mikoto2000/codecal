import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import packageInfo from "./package.json" with { type: "json" };
export default defineConfig({
  define: { __APP_VERSION__: JSON.stringify(packageInfo.version) },
  plugins: [react(), tailwindcss()],
  server: { port: 1420, strictPort: true },
  clearScreen: false,
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test-setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
