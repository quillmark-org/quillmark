#import "@local/quillmark-helper:0.1.0": data

// The whole point of this fixture: `$body` carries a GFM table, which the
// markdown -> Content -> Typst path lowers to `#table(...)`. Rendering the
// body end-to-end is the coverage — alignment, formatted cells, and ragged
// rows all reach the emitter here, not just the inline codec's unit tests.
#underline(data.title)

#data.at("$body")
