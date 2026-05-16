import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const enc = new TextEncoder()

// Minimal font shipped with quillmark fixtures, loaded once. The Typst world
// rejects compilation when no fonts are present, so every test quill needs at
// least one — quills are responsible for shipping their own fonts now that
// quillmark-typst no longer embeds a default fallback.
const __dirname = dirname(fileURLToPath(import.meta.url))
const TEST_FONT_PATH = join(
  __dirname,
  '../../fixtures/resources/quills/usaf_memo/0.1.0/packages/tonguetoquill-usaf-memo/fonts/CopperplateCC/CopperplateCC-Heavy.otf',
)
const TEST_FONT_BYTES = new Uint8Array(readFileSync(TEST_FONT_PATH))

export function makeQuill({
  name = 'test_quill',
  version = '1.0.0',
  main = '#import "@local/quillmark-helper:0.1.0": data\n= Test',
  quillYaml,
} = {}) {
  const yaml = quillYaml ?? `quill:
  name: ${name}
  version: "${version}"
  backend: typst
  main_file: main.typ
  description: Test quill for smoke tests
`
  return new Map([
    ['Quill.yaml', enc.encode(yaml)],
    ['main.typ', enc.encode(main)],
    ['assets/fonts/test.otf', TEST_FONT_BYTES],
  ])
}
