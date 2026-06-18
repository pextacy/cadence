import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  // Relative asset paths so the production build works when opened directly
  // (file://) or served from any sub-path, not just the domain root.
  base: "./",
  plugins: [react()],
  server: { port: 5173 },
});
