import { defineConfig } from "vite";
import { fileURLToPath } from "node:url";
export default defineConfig({
  server: {
    fs: {
      allow: [
        fileURLToPath(new URL(".", import.meta.url)),
        fileURLToPath(new URL("../sdk/ts/src", import.meta.url)),
      ],
    },
    port: 5173,
    strictPort: true,
    proxy: {
      "/v1": {
        target: process.env.ALIGHT_API_ORIGIN ?? "http://127.0.0.1:8080",
        ws: true,
      },
    },
  },
  build: { target: "es2022", sourcemap: false },
});
