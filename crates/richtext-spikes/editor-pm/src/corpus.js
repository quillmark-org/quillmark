/** @typedef {{ start: number, end: number, type: string, attrs?: Record<string, string> }} CorpusMark */

const KIND_ORD = {
  strong: 0,
  emph: 1,
  underline: 2,
  strike: 3,
  code: 4,
  link: 5,
  anchor: 6,
};

/** USV length — one astral char counts as 1. */
export function usvLen(text) {
  let n = 0;
  for (const _ of text) n++;
  return n;
}

/** Map corpus mark `type` to ProseMirror mark name. */
export function pmMarkName(type) {
  if (type === "emph") return "em";
  return type;
}

/** Map ProseMirror mark name to corpus `type`. */
export function corpusMarkType(name) {
  if (name === "em") return "emph";
  return name;
}

function attrsKey(type, attrs) {
  if (type === "link" && attrs?.href) return attrs.href;
  return "";
}

/**
 * Phase-1 normalization (formatting marks only): union same-kind adjacent/overlap,
 * drop zero-width formatting, trim `\n` edges, canonical sort.
 * @param {string} text
 * @param {CorpusMark[]} marks
 * @returns {CorpusMark[]}
 */
export function normalizeFormattingMarks(text, marks) {
  const chars = [...text];
  const trimmed = marks
    .filter((m) => m.type !== "anchor")
    .map((m) => {
      let { start, end } = m;
      while (start < end && chars[start] === "\n") start++;
      while (end > start && chars[end - 1] === "\n") end--;
      return { ...m, start, end };
    })
    .filter((m) => m.start < m.end);

  /** @type {Map<string, { type: string, attrs?: Record<string, string>, ranges: [number, number][] }>} */
  const groups = new Map();
  const passthrough = [];

  for (const m of trimmed) {
    if (m.type === "anchor") {
      passthrough.push(m);
      continue;
    }
    const key = `${KIND_ORD[m.type] ?? 99}:${attrsKey(m.type, m.attrs)}`;
    let g = groups.get(key);
    if (!g) {
      g = { type: m.type, attrs: m.attrs, ranges: [] };
      groups.set(key, g);
    }
    g.ranges.push([m.start, m.end]);
  }

  /** @type {CorpusMark[]} */
  const out = [];
  for (const g of groups.values()) {
    g.ranges.sort((a, b) => a[0] - b[0]);
    let [cs, ce] = g.ranges[0];
    for (const [s, e] of g.ranges.slice(1)) {
      if (s <= ce) ce = Math.max(ce, e);
      else {
        out.push({ start: cs, end: ce, type: g.type, attrs: g.attrs });
        [cs, ce] = [s, e];
      }
    }
    out.push({ start: cs, end: ce, type: g.type, attrs: g.attrs });
  }
  out.push(...passthrough);

  out.sort((a, b) => {
    const oa = KIND_ORD[a.type] ?? 99;
    const ob = KIND_ORD[b.type] ?? 99;
    return (
      a.start - b.start ||
      a.end - b.end ||
      oa - ob ||
      attrsKey(a.type, a.attrs).localeCompare(attrsKey(b.type, b.attrs))
    );
  });
  return out;
}

/**
 * Inline `richtext(inline)` corpus shell — one empty para line, no islands.
 * @param {string} text
 * @param {CorpusMark[]} marks
 */
export function inlineCorpus(text, marks) {
  const normalized = normalizeFormattingMarks(text, marks);
  return {
    text,
    lines: [{ kind: "para", containers: [], continues: false }],
    marks: normalized.map(({ start, end, type, attrs }) => {
      if (type === "link") {
        return { start, end, type: "link", url: attrs?.href ?? attrs?.url ?? "" };
      }
      return { start, end, type };
    }),
    islands: [],
  };
}
