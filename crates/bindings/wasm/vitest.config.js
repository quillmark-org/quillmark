import { defineConfig } from 'vitest/config'
import wasm from 'vite-plugin-wasm'
import topLevelAwait from 'vite-plugin-top-level-await'
import path from 'path'
import { fileURLToPath } from 'url'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)

// Centralized workspace root and bundle paths. `@quillmark-wasm` aliases the
// Typst backend binary directly (the API superset the basic/canvas suites
// exercise — it is NOT a public package export), `@quillmark-wasm/core` the
// Typst-less core build, and `@quillmark-wasm/runtime` the hand-written
// canonical layer (the package's public root). NOTE: neither `@quillmark-wasm`
// nor `@quillmark-wasm/core` is a public package subpath — the package exposes
// exactly ONE entry point (the root). These aliases reach internal build
// artifacts so the bundle suites (`core.test.js`/`basic.test.js`/
// `canvas.test.js`) can exercise them directly.
export const WORKSPACE_ROOT = path.resolve(__dirname, '..', '..', '..')
export const WASM_BUNDLE_PATH = path.join(WORKSPACE_ROOT, 'pkg', 'backends', 'typst', 'wasm.js')
export const WASM_PDFFORM_BUNDLE_PATH = path.join(WORKSPACE_ROOT, 'pkg', 'backends', 'pdfform', 'wasm.js')
export const WASM_CORE_BUNDLE_PATH = path.join(WORKSPACE_ROOT, 'pkg', 'core', 'wasm.js')
export const WASM_RUNTIME_BUNDLE_PATH = path.join(WORKSPACE_ROOT, 'pkg', 'runtime', 'runtime.js')

export default defineConfig({
  plugins: [wasm(), topLevelAwait()],
  resolve: {
    alias: {
      // More specific first: rollup alias matches `find` followed by `/` or end,
      // so `@quillmark-wasm/{core,runtime,pdfform}` must precede the `@quillmark-wasm` prefix.
      '@quillmark-wasm/runtime': WASM_RUNTIME_BUNDLE_PATH,
      '@quillmark-wasm/pdfform': WASM_PDFFORM_BUNDLE_PATH,
      '@quillmark-wasm/core': WASM_CORE_BUNDLE_PATH,
      '@quillmark-wasm': WASM_BUNDLE_PATH,
    },
  },
  test: {
    environment: 'node',
    testTimeout: 40000,
  },
})
