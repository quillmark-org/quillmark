import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import wasm from "vite-plugin-wasm";
import topLevelAwait from "vite-plugin-top-level-await";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "../../..");
const WASM_RUNTIME = path.join(WORKSPACE_ROOT, "pkg", "runtime", "runtime.js");

// editor-pm is imported as source via @editor-pm; it has its own node_modules, so
// without pinning every prosemirror-* import here Vite loads two copies of
// prosemirror-model (schema from one, EditorView/DOMParser from the other).
const PM_PACKAGES = [
  "prosemirror-model",
  "prosemirror-state",
  "prosemirror-transform",
  "prosemirror-view",
  "prosemirror-commands",
  "prosemirror-keymap",
  "prosemirror-history",
  "prosemirror-schema-basic",
];

/** @type {Record<string, string>} */
const prosemirrorAliases = Object.fromEntries(
  PM_PACKAGES.map((pkg) => [pkg, path.resolve(__dirname, "node_modules", pkg)])
);

export default defineConfig({
  plugins: [wasm(), topLevelAwait()],
  resolve: {
    alias: {
      "@quillmark/runtime": WASM_RUNTIME,
      "@editor-pm": path.resolve(__dirname, "../editor-pm/src"),
      ...prosemirrorAliases,
    },
    dedupe: PM_PACKAGES,
  },
  optimizeDeps: {
    include: PM_PACKAGES,
  },
  server: {
    fs: { allow: [WORKSPACE_ROOT] },
    port: 5173,
  },
});
