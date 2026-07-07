import { describe, expect, it } from "vitest";
import { normalizeFormattingMarks } from "../src/corpus.js";
import { corpusToDoc, docToCorpusMarks, EditorSession } from "../src/bridge.js";

describe("Spike-A — ProseMirror mark binding", () => {
  it("round-trips strong+emph overlap without model change", () => {
    const text = "hello";
    const marks = [
      { start: 0, end: 5, type: "strong" },
      { start: 1, end: 4, type: "emph" },
    ];
    const doc = corpusToDoc(text, marks);
    const out = docToCorpusMarks(doc);
    expect(out.text).toBe(text);
    expect(out.marks).toEqual(normalizeFormattingMarks(text, marks));
  });

  it("edge-expand: typing at trailing strong edge expands stored mark", () => {
    const session = new EditorSession("hel", [{ start: 0, end: 3, type: "strong" }]);
    session.setCursor(3);
    session.insert("o");
    const { text, marks } = session.exportMarks();
    expect(text).toBe("helo");
    // PM expands strong across the typed char — corpus stores the union.
    expect(marks).toEqual([{ start: 0, end: 4, type: "strong" }]);
    // Re-load into a fresh session is stable.
    const again = new EditorSession(text, marks).exportMarks();
    expect(again).toEqual({ text, marks });
  });

  it("adjacent-merge: typing at strong edge extends the range", () => {
    const session = new EditorSession("a", [{ start: 0, end: 1, type: "strong" }]);
    session.setCursor(1);
    session.insert("b");
    const { text, marks } = session.exportMarks();
    expect(text).toBe("ab");
    expect(marks).toEqual([{ start: 0, end: 2, type: "strong" }]);
  });

  it("cross-kind overlap: strong and emph on same span coexist", () => {
    const session = new EditorSession("hi", []);
    session.state = session.state.apply(
      session.state.tr.setSelection(
        // select USV 0..2 inside paragraph
        session.state.selection.constructor.create(session.state.doc, 1, 3)
      )
    );
    session.toggleMark("strong");
    session.toggleMark("emph");
    const { marks } = session.exportMarks();
    expect(marks).toEqual([
      { start: 0, end: 2, type: "strong" },
      { start: 0, end: 2, type: "emph" },
    ]);
  });

  it("boundary split: delete at mark edge trims strong", () => {
    const session = new EditorSession("abc", [{ start: 0, end: 3, type: "strong" }]);
    session.setCursor(3);
    session.deleteBackward();
    const { text, marks } = session.exportMarks();
    expect(text).toBe("ab");
    expect(marks).toEqual([{ start: 0, end: 2, type: "strong" }]);
  });

  it("inline corpus shape: single para, no islands", () => {
    const corpus = new EditorSession("x", []).exportCorpus();
    expect(corpus.lines).toEqual([{ kind: "para", containers: [], continues: false }]);
    expect(corpus.islands).toEqual([]);
    expect(corpus.marks).toEqual([]);
  });

  it("astral char: USV count is 1 per astral (bridge uses USV, not UTF-16)", () => {
    const text = "a😀b";
    expect([...text].length).toBe(3);
    const session = new EditorSession(text, [{ start: 1, end: 2, type: "emph" }]);
    const { marks } = session.exportMarks();
    expect(marks).toEqual([{ start: 1, end: 2, type: "emph" }]);
  });
});
