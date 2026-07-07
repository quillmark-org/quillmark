import { Quill, Document, Engine, init } from "@quillmark/runtime";
import { docToCorpusMarks } from "@editor-pm/bridge.js";
import { loadUsafMemoTree } from "./load-usaf-memo.js";
import { corpusToEditorInput, editorExportToCorpus } from "./corpus-bridge.js";
import { mountBodyField } from "./body-field.js";
import { mountRichtextField, wireToolbar } from "./richtext-field.js";
import { findFieldRegion, paintPage, positionRegionHighlight } from "./preview.js";
import { regionCenterOnCanvas, wireCanvasNavigation } from "./navigation.js";
import { fieldValue } from "./fields.js";

const statusEl = document.getElementById("status");
const canvas = /** @type {HTMLCanvasElement} */ (document.getElementById("page-canvas"));
const previewPane = /** @type {HTMLElement} */ (document.querySelector(".preview-pane"));
const highlightEl = /** @type {HTMLElement} */ (document.getElementById("region-highlight"));
const regionsDump = /** @type {HTMLElement} */ (document.getElementById("regions-dump"));
const bodyHost = /** @type {HTMLElement} */ (document.getElementById("body-editor"));
const subjectHost = /** @type {HTMLElement} */ (document.getElementById("subject-editor"));
const subjectToolbar = /** @type {HTMLElement} */ (document.getElementById("subject-toolbar"));
const tagLineHost = /** @type {HTMLElement} */ (document.getElementById("tag-line-editor"));
const tagLineToolbar = /** @type {HTMLElement} */ (document.getElementById("tag-line-toolbar"));

/** Test hook — Playwright waits on this instead of timing out on status text. */
window.__POC__ = {
  ready: false,
  error: null,
  get activeField() {
    return activeField;
  },
  /** Canvas-local click point at a field region center (for e2e). */
  regionClickPoint(field) {
    if (!session) return null;
    const region = findFieldRegion(session.regions(), field);
    if (!region) return null;
    return regionCenterOnCanvas(canvas, session.pageSize(0), region);
  },
};

/** @type {import('@quillmark/runtime').LiveSession | null} */
let session = null;
/** @type {InstanceType<typeof Document> | null} */
let doc = null;
/** @type {Record<string, ReturnType<typeof mountRichtextField>>} */
const fields = {};
/** @type {string | null} */
let activeField = null;
let applyTimer = 0;
let painting = false;

function setStatus(text, kind = "") {
  statusEl.textContent = text;
  statusEl.className = `status ${kind}`.trim();
}

function seedMemoDocument(/** @type {InstanceType<typeof Quill>} */ quill) {
  const d = quill.seedDocument();
  d.setFields({
    memo_for: ["HQ USAF/A1"],
    subject: "**Richtext** form POC",
    signature_block: ["JANE DOE, Col, USAF", "Director of Testing"],
    letterhead_caption: ["HEADQUARTERS EXAMPLE WING"],
    tag_line: "**Semper** *Supra*",
  });
  d.replaceBody(
    "The first paragraph. Top-level paragraphs are auto-numbered; do not add manual numbering.\n\n- Nested bullets are automatically lettered."
  );
  return d;
}

async function commitDocument(/** @type {string} */ reason) {
  if (!session || !doc || !fields.$body || !fields.subject || !fields.tag_line) return;
  if (painting) return;

  doc.replaceBody(fields.$body.getMarkdown());
  doc.setField("subject", editorExportToCorpus(docToCorpusMarks(fields.subject.view.state.doc)));
  doc.setField("tag_line", editorExportToCorpus(docToCorpusMarks(fields.tag_line.view.state.doc)));

  painting = true;
  try {
    const t0 = performance.now();
    const cs = session.apply(doc);
    paintPage(session, 0, canvas);
    updateOverlays();
    const ms = Math.round(performance.now() - t0);
    setStatus(`Applied (${reason}) — ${ms}ms, dirty pages: [${cs.dirtyPages.join(", ")}]`, "ok");
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    setStatus(`Apply failed: ${msg}`, "err");
    console.error(e);
  } finally {
    painting = false;
  }
}

function scheduleApply(reason) {
  clearTimeout(applyTimer);
  applyTimer = window.setTimeout(() => commitDocument(reason), 280);
}

function setActiveField(field) {
  activeField = field;
  updateOverlays();
}

function updateOverlays() {
  if (!session) return;
  const regions = session.regions();
  regionsDump.textContent = JSON.stringify(regions, null, 2);
  const target = activeField ?? "subject";
  const region = findFieldRegion(regions, target);
  if (region) {
    positionRegionHighlight(highlightEl, region, session.pageSize(0), previewPane, target);
  } else {
    highlightEl.hidden = true;
    delete highlightEl.dataset.field;
  }
}

async function boot() {
  try {
    setStatus("Initializing WASM…");
    await init();

    setStatus("Loading usaf_memo quill…");
    const tree = await loadUsafMemoTree();
    const quill = Quill.fromTree(tree);

    doc = seedMemoDocument(quill);

    const engine = new Engine();
    setStatus("Opening live session (first compile may take a few seconds)…");
    session = await engine.open(quill, doc);

    paintPage(session, 0, canvas);

    fields.$body = mountBodyField(bodyHost, doc.main.bodyMarkdown, () => scheduleApply("$body"));
    fields.$body.el.addEventListener("focusin", () => setActiveField("$body"));

    fields.subject = mountRichtextField(
      subjectHost,
      corpusToEditorInput(fieldValue(doc.main, "subject")),
      () => scheduleApply("subject")
    );
    wireToolbar(subjectToolbar, fields.subject);
    fields.subject.view.dom.addEventListener("focusin", () => setActiveField("subject"));

    fields.tag_line = mountRichtextField(
      tagLineHost,
      corpusToEditorInput(fieldValue(doc.main, "tag_line")),
      () => scheduleApply("tag_line")
    );
    wireToolbar(tagLineToolbar, fields.tag_line);
    fields.tag_line.view.dom.addEventListener("focusin", () => setActiveField("tag_line"));

    activeField = "subject";
    updateOverlays();

    wireCanvasNavigation(canvas, () => session, fields, setActiveField);

    window.addEventListener("resize", () => {
      if (session) {
        paintPage(session, 0, canvas);
        updateOverlays();
      }
    });

    setStatus("Ready — edit fields or click the preview to cross-navigate.", "ok");
    window.__POC__.ready = true;
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    setStatus(`Boot failed: ${msg}`, "err");
    window.__POC__.error = msg;
    console.error(e);
  }
}

boot();
