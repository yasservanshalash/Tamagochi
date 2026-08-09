import { defineConfig } from "vite";
import { resolve } from "node:path";

// The web surfaces of Deskfolk. The companion itself is a native layered window
// (`deskfolk-render-win`) — WebView2's host draws caption buttons no amount of
// window surgery removes — but the rectangular, mostly-opaque screens are web
// pages: the first-run onboarding wizard (`wizard.html`) and the Control Center
// (`index.html`). Each is its own Tauri window opened from Rust.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/target/**", "**/characters/**"] },
  },
  build: {
    target: "esnext",
    emptyOutDir: true,
    rollupOptions: {
      input: {
        index: resolve(__dirname, "index.html"),
        wizard: resolve(__dirname, "wizard.html"),
        spritestudio: resolve(__dirname, "spritestudio.html"),
      },
    },
  },
});
