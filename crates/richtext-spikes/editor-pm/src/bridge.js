import { Fragment, Node } from "prosemirror-model";
import { EditorState, TextSelection } from "prosemirror-state";
import { ReplaceStep } from "prosemirror-transform";
import {
  corpusMarkType,
  inlineCorpus,
  normalizeFormattingMarks,
  pmMarkName,
  usvLen,
} from "./corpus.js";
import { schema } from "./schema.js";

/** @typedef {import('./corpus.js').CorpusMark} CorpusMark */

/**
 * Build a single-paragraph ProseMirror doc from inline corpus text + marks.
 * @param {string} text
 * @param {CorpusMark[]} marks
 */
export function corpusToDoc(text, marks) {
  const normalized = normalizeFormattingMarks(text, marks);
  if (text.length === 0) {
    return schema.node("doc", null, [schema.node("paragraph")]);
  }

  /** @type {{ from: number, to: number, mark: import('prosemirror-model').Mark }[]} */
  const intervals = normalized.map((m) => {
    const name = pmMarkName(m.type);
    const spec = schema.marks[name];
    if (!spec) throw new Error(`unknown mark: ${m.type}`);
    const attrs =
      m.type === "link" ? { href: m.attrs?.href ?? m.attrs?.url ?? "" } : null;
    return {
      from: m.start,
      to: m.end,
      mark: spec.create(attrs),
    };
  });

  /** @type {import('prosemirror-model').Mark[]} */
  const marksAt = (pos) =>
    intervals.filter((i) => pos >= i.from && pos < i.to).map((i) => i.mark);

  const chars = [...text];
  const content = [];
  for (let i = 0; i < chars.length; i++) {
    content.push(schema.text(chars[i], marksAt(i)));
  }
  const para = schema.node("paragraph", null, Fragment.fromArray(content));
  return schema.node("doc", null, [para]);
}

/**
 * Extract formatting marks from a ProseMirror doc as USV ranges.
 * @param {Node} doc
 * @returns {{ text: string, marks: CorpusMark[] }}
 */
export function docToCorpusMarks(doc) {
  const para = doc.firstChild;
  if (!para) return { text: "", marks: [] };

  let text = "";
  /** @type {CorpusMark[]} */
  const raw = [];

  para.forEach((node, offset) => {
    if (!node.isText) return;
    const base = usvLen(text);
    const slice = node.text ?? "";
    const sliceChars = [...slice];
    for (let i = 0; i < sliceChars.length; i++) {
      const usv = base + i;
      for (const mark of node.marks) {
        raw.push({
          start: usv,
          end: usv + 1,
          type: corpusMarkType(mark.type.name),
          attrs: mark.type.name === "link" ? { href: mark.attrs.href } : undefined,
        });
      }
    }
    text += slice;
  });

  return { text, marks: normalizeFormattingMarks(text, raw) };
}

/**
 * Headless editor session for Spike-A scenarios.
 */
export class EditorSession {
  /** @param {string} text @param {CorpusMark[]} marks */
  constructor(text, marks) {
    this.doc = corpusToDoc(text, marks);
    this.state = EditorState.create({ doc: this.doc, schema });
  }

  /** @returns {{ text: string, marks: CorpusMark[] }} */
  exportMarks() {
    return docToCorpusMarks(this.state.doc);
  }

  /** @returns {ReturnType<typeof inlineCorpus>} */
  exportCorpus() {
    const { text, marks } = this.exportMarks();
    return inlineCorpus(text, marks);
  }

  /** Move cursor to USV offset inside the single paragraph. */
  setCursor(usv) {
    const pos = usvToPmPos(this.state.doc, usv);
    this.state = this.state.apply(
      this.state.tr.setSelection(TextSelection.create(this.state.doc, pos))
    );
  }

  /** Insert `str` at cursor; PM stored marks apply (edge-expand behavior). */
  insert(str) {
    const { from } = this.state.selection;
    const tr = this.state.tr.insertText(str, from);
    this.state = this.state.apply(tr);
  }

  /** Toggle a mark over the current selection (empty selection = stored marks). */
  toggleMark(type) {
    const name = pmMarkName(type);
    const markType = schema.marks[name];
    if (!markType) throw new Error(`unknown mark: ${type}`);
    const { from, to, empty } = this.state.selection;
    let tr = this.state.tr;
    if (empty) {
      const has = markType.isInSet(this.state.storedMarks || this.state.selection.$from.marks());
      tr = has ? tr.removeStoredMark(markType) : tr.addStoredMark(markType.create());
    } else {
      const has = this.state.doc.rangeHasMark(from, to, markType);
      tr = has
        ? tr.removeMark(from, to, markType)
        : tr.addMark(from, to, markType.create());
    }
    this.state = this.state.apply(tr);
  }

  /** Delete one USV char before cursor. */
  deleteBackward() {
    const { from } = this.state.selection;
    if (from <= 1) return;
    const tr = this.state.tr.delete(from - 1, from);
    this.state = this.state.apply(tr);
  }

  /**
   * Text delta from initial doc to current — retain/insert/delete in USV.
   * Coarse (one replace) suffices for Spike-A; phase-3 PR-B brings Myers/LCS.
   */
  textDelta(baseText) {
    const { text: newText } = this.exportMarks();
    return diffUsv(baseText, newText);
  }
}

/** Map USV offset to a ProseMirror doc position inside the inline paragraph. */
export function usvToPmPos(doc, targetUsv) {
  const para = doc.firstChild;
  if (!para) return 1;
  let usv = 0;
  let pos = 1;
  for (let i = 0; i < para.childCount; i++) {
    const node = para.child(i);
    if (!node.isText) continue;
    const nodeUsv = [...(node.text ?? "")].length;
    if (targetUsv <= usv + nodeUsv) {
      return pos + (targetUsv - usv);
    }
    usv += nodeUsv;
    pos += node.nodeSize;
  }
  return 1 + para.content.size;
}

/** Prefix/suffix trim diff — mirrors phase-1 `delta::diff`. */
export function diffUsv(base, next) {
  const a = [...base];
  const b = [...next];
  let p = 0;
  while (p < a.length && p < b.length && a[p] === b[p]) p++;
  let s = 0;
  while (s < a.length - p && s < b.length - p && a[a.length - 1 - s] === b[b.length - 1 - s]) {
    s++;
  }
  /** @type {{ op: string, n?: number, s?: string }[]} */
  const ops = [];
  if (p > 0) ops.push({ op: "retain", n: p });
  const del = a.length - p - s;
  if (del > 0) ops.push({ op: "delete", n: del });
  const ins = b.slice(p, b.length - s).join("");
  if (ins) ops.push({ op: "insert", s: ins });
  if (s > 0) ops.push({ op: "retain", n: s });
  return ops;
}

/** Apply a ReplaceStep and return whether PM accepted it. */
export function applyReplaceStep(state, from, to, text) {
  const step = new ReplaceStep(from, to, schema.text(text));
  const result = step.apply(state.doc);
  if (result.failed) return null;
  return state.apply(state.tr.step(step));
}
