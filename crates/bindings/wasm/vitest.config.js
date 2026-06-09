import { defineConfig } from 'vitest/config'
import wasm from 'vite-plugin-wasm'
import topLevelAwait from 'vite-plugin-top-level-await'
import path from 'path'
import { fileURLToPath } from 'url'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)

// Centralized workspace root and bundle paths. The split ships two artifacts;
// `@quillmark-wasm` aliases the render build (the API superset the existing
// suite exercises), and `@quillmark-wasm/core` the Typst-less core build.
export const WORKSPACE_ROOT = path.resolve(__dirname, '..', '..', '..')
export const WASM_BUNDLE_PATH = path.join(WORKSPACE_ROOT, 'pkg', 'render', 'wasm.js')
export const WASM_CORE_BUNDLE_PATH = path.join(WORKSPACE_ROOT, 'pkg', 'core', 'wasm.js')
export const WASM_RUNTIME_BUNDLE_PATH = path.join(WORKSPACE_ROOT, 'pkg', 'runtime', 'runtime.js')

export default defineConfig({
  plugins: [wasm(), topLevelAwait()],
  resolve: {
    alias: {
      // More specific first: rollup alias matches `find` followed by `/` or end,
      // so `@quillmark-wasm/{core,runtime}` must precede the `@quillmark-wasm` prefix.
      '@quillmark-wasm/runtime': WASM_RUNTIME_BUNDLE_PATH,
      '@quillmark-wasm/core': WASM_CORE_BUNDLE_PATH,
      '@quillmark-wasm': WASM_BUNDLE_PATH,
    },
  },
  test: {
    environment: 'node',
    testTimeout: 40000,
  },
})
