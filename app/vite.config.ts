import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The window loads this page from a local port while it is being developed,
// and from files inside the app once it is built.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1430, strictPort: true, host: "127.0.0.1" },
  build: { target: "safari15", outDir: "dist", chunkSizeWarningLimit: 1500 },
});
