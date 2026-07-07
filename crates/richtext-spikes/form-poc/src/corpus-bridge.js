import { inlineCorpus } from "@editor-pm/corpus.js";

/**
 * Inline markdown → editor input. Spike-only: `setField` stores authored markdown
 * until compile coerces it; the editor needs USV text + marks now.
 * @param {string} md
 */
export function markdownInlineToEditor(md) {
  if (!md) return { text: "", marks: [] };
  /** @type {import('@editor-pm/corpus.js').CorpusMark[]} */
  const marks = [];
  let text = "";
  let i = 0;
  while (i < md.length) {
    if (md.startsWith("**", i)) {
      const end = md.indexOf("**", i + 2);
      if (end === -1) {
        text += md[i++];
        continue;
      }
      const start = text.length;
      text += md.slice(i + 2, end);
      marks.push({ start, end: text.length, type: "strong" });
      i = end + 2;
    } else if (md[i] === "*") {
      const end = md.indexOf("*", i + 1);
      if (end === -1) {
        text += md[i++];
        continue;
      }
      const start = text.length;
      text += md.slice(i + 1, end);
      marks.push({ start, end: text.length, type: "emph" });
      i = end + 1;
    } else {
      text += md[i++];
    }
  }
  return { text, marks };
}

/**
 * WASM field value → editor input. Accepts canonical corpus JSON or authored
 * markdown (pre-coercion payload shape).
 * @param {unknown} value
 */
export function corpusToEditorInput(value) {
  if (typeof value === "string") return markdownInlineToEditor(value);
  if (!value || typeof value !== "object") {
    return { text: "", marks: [] };
  }
  const c = /** @type {{ text?: string, marks?: Array<{ start: number, end: number, kind: Record<string, unknown> }> }} */ (
    value
  );
  const text = c.text ?? "";
  const marks = (c.marks ?? []).flatMap((m) => {
    const type = /** @type {string} */ (m.type ?? m.kind?.type ?? "");
    if (!type || type === "anchor") return [];
    if (type === "link") {
      const url = m.url ?? m.kind?.url ?? "";
      return [{ start: m.start, end: m.end, type: "link", attrs: { href: url } }];
    }
    if (type === "emph") return [{ start: m.start, end: m.end, type: "emph" }];
    return [{ start: m.start, end: m.end, type }];
  });
  return { text, marks };
}

/** Editor export → canonical corpus for Document.setField. */
export function editorExportToCorpus(/** @type {{ text: string, marks: import('@editor-pm/corpus.js').CorpusMark[] }} */ exp) {
  return inlineCorpus(exp.text, exp.marks);
}

/** Markdown projection for debug display. */
export function corpusToMarkdownPreview(corpus) {
  const { text, marks } = corpusToEditorInput(corpus);
  if (!text) return "(empty)";
  // Minimal inline markdown hint — not a full exporter.
  return marks.length ? `${text} [${marks.map((m) => m.type).join(",")}]` : text;
}
