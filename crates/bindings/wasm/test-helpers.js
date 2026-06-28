import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const enc = new TextEncoder()

// Minimal font shipped with quillmark fixtures, loaded once. The Typst world
// rejects compilation when no fonts are present, so every test quill needs at
// least one — quills are responsible for shipping their own fonts, since
// quillmark-typst embeds no default fallback.
const __dirname = dirname(fileURLToPath(import.meta.url))
const TEST_FONT_PATH = join(
  __dirname,
  '../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/fonts/CopperplateCC/CopperplateCC-Heavy.otf',
)
const TEST_FONT_BYTES = new Uint8Array(readFileSync(TEST_FONT_PATH))

export function makeQuill({
  name = 'test_quill',
  version = '1.0.0',
  plate = '#import "@local/quillmark-helper:0.1.0": data\n= Test',
  quillYaml,
} = {}) {
  const yaml = quillYaml ?? `quill:
  name: ${name}
  version: "${version}"
  backend: typst
  description: Test quill for smoke tests

typst:
  plate_file: plate.typ
`
  return new Map([
    ['Quill.yaml', enc.encode(yaml)],
    ['plate.typ', enc.encode(plate)],
    ['assets/fonts/test.otf', TEST_FONT_BYTES],
  ])
}

// The hand-authored `sample_form` fixture: a `pdfform`-backend quill shipping a
// stripped background (`form.pdf`) and a value-free field spec (`form.json`).
// Loaded as a tree so the canvas tests can drive the pdfform-preview backend
// (which rasterizes the pre-flattened page) exactly like a typst quill.
const SAMPLE_FORM_DIR = join(__dirname, '../../fixtures/resources/quills/sample_form/0.1.0')

export function makeSampleFormQuill() {
  return new Map([
    ['Quill.yaml', new Uint8Array(readFileSync(join(SAMPLE_FORM_DIR, 'Quill.yaml')))],
    ['form.pdf', new Uint8Array(readFileSync(join(SAMPLE_FORM_DIR, 'form.pdf')))],
    ['form.json', new Uint8Array(readFileSync(join(SAMPLE_FORM_DIR, 'form.json')))],
  ])
}

// A filled sample_form document: binds the FullName text field (among others), so
// the pre-flattened raster carries visible field-value ink.
export const SAMPLE_FORM_MARKDOWN = `~~~
$quill: sample_form
$kind: main
full_name: Ada Lovelace
comments:
  - First comment line.
  - Second comment line.
agree: true
favorite_color: green
~~~
`
