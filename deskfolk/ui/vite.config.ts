import { defineConfig } from "vite";
import { resolve } from "node:path";

// One entry point, one window. The companion used to be the second: a
// transparent webview drawing into a canvas. It is now a native layered window
// (`deskfolk-render-win`), because WebView2's host draws caption buttons that
// no amount of window surgery removes. Only the Control Center is a web page.
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
      },
    },
  },
});
