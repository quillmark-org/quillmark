import { EditorState, TextSelection } from "prosemirror-state";
import { EditorView } from "prosemirror-view";
import { keymap } from "prosemirror-keymap";
import { history, undo, redo } from "prosemirror-history";
import { baseKeymap, toggleMark } from "prosemirror-commands";
import { schema } from "@editor-pm/schema.js";
import { corpusToDoc, docToCorpusMarks, usvToPmPos } from "@editor-pm/bridge.js";
import { pmMarkName } from "@editor-pm/corpus.js";

/**
 * Mount a single-paragraph ProseMirror field.
 * @param {HTMLElement} host
 * @param {{ text: string, marks: import('@editor-pm/corpus.js').CorpusMark[] }} initial
 * @param {(exportMarks: ReturnType<typeof docToCorpusMarks>) => void} onChange
 */
export function mountRichtextField(host, initial, onChange) {
  const doc = corpusToDoc(initial.text, initial.marks);
  let suppress = false;

  const state = EditorState.create({
    doc,
    schema,
    plugins: [
      history(),
      keymap({
        "Mod-z": undo,
        "Mod-y": redo,
        "Mod-Shift-z": redo,
        ...Object.fromEntries(
          ["strong", "emph", "underline"].map((t) => {
            const name = pmMarkName(t);
            const mark = schema.marks[name];
            return [`Mod-${t === "strong" ? "b" : t === "emph" ? "i" : "u"}`, toggleMark(mark)];
          })
        ),
      }),
      keymap(baseKeymap),
    ],
  });

  const view = new EditorView(host, {
    state,
    dispatchTransaction(tr) {
      const next = view.state.applyTransaction(tr);
      view.updateState(next.state);
      if (suppress || !tr.docChanged) return;
      onChange(docToCorpusMarks(next.state.doc));
    },
  });

  return {
    view,
    /** Replace document without firing onChange. */
    setContent(/** @type {{ text: string, marks: import('@editor-pm/corpus.js').CorpusMark[] }} */ content) {
      suppress = true;
      const d = corpusToDoc(content.text, content.marks);
      view.updateState(EditorState.create({ doc: d, schema, plugins: view.state.plugins }));
      suppress = false;
    },
    toggleMark(type) {
      const name = pmMarkName(type);
      const markType = schema.marks[name];
      if (!markType) return;
      toggleMark(markType)(view.state, view.dispatch);
      view.focus();
    },
    focus() {
      view.focus();
    },
    /** Place caret at USV offset inside the inline paragraph. */
    setCursor(usv) {
      suppress = true;
      const pos = usvToPmPos(view.state.doc, usv);
      view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, pos)));
      suppress = false;
      view.focus();
    },
    destroy() {
      view.destroy();
    },
  };
}

/**
 * Wire toolbar buttons to toggle marks; toggles `.active` from selection.
 * @param {HTMLElement} toolbar
 * @param {ReturnType<typeof mountRichtextField>} field
 */
export function wireToolbar(toolbar, field) {
  const marks = ["strong", "emph", "underline"];
  const buttons = [...toolbar.querySelectorAll("[data-mark]")];

  const refresh = () => {
    for (const btn of buttons) {
      const type = btn.getAttribute("data-mark");
      const name = pmMarkName(type);
      const markType = schema.marks[name];
      const active = markType?.isInSet(field.view.state.storedMarks || field.view.state.selection.$from.marks());
      btn.classList.toggle("active", !!active);
    }
  };

  field.view.dom.addEventListener("focusin", refresh);
  field.view.dom.addEventListener("keyup", refresh);
  field.view.dom.addEventListener("mouseup", refresh);

  for (const btn of buttons) {
    btn.addEventListener("click", () => {
      field.toggleMark(btn.getAttribute("data-mark"));
      refresh();
    });
  }
}
