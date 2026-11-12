import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The port is fixed because the Tauri shell points at it in development.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: "es2022", outDir: "dist" },
});
