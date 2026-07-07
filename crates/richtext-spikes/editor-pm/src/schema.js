import { Schema } from "prosemirror-model";
import { schema as basicSchema } from "prosemirror-schema-basic";

/** Frozen formatting mark set (phase-1) as ProseMirror marks. */
const marks = basicSchema.spec.marks
  .addToEnd("underline", {
    parseDOM: [{ tag: "u" }],
    toDOM() {
      return ["u", 0];
    },
  })
  .addToEnd("strike", {
    parseDOM: [{ tag: "s" }, { tag: "del" }],
    toDOM() {
      return ["s", 0];
    },
  });

export const schema = new Schema({
  nodes: basicSchema.spec.nodes,
  marks,
});
