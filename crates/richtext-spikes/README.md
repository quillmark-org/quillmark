# richtext-spikes

Throwaway probes for the richtext rework (`prose/plans/richtext/`). Workspace
member, **outside** `default-members` — nothing here ships in the product.

| Probe | Path | Gate |
|-------|------|------|
| Spike-A (phase 3) | `editor-pm/` | Real rich-editor binding — ProseMirror mark semantics vs corpus freeze |
| Form POC (phase 3 PR-H) | `form-poc/` | Browser manual test — canvas preview + ProseMirror `tag_line` field |

Run Spike-A:

```bash
cd crates/richtext-spikes/editor-pm && npm install && npm test
```

Run the form POC (manual UI):

```bash
cd crates/richtext-spikes/form-poc && npm install && npm run dev
```

Then open the URL Vite prints (default `http://localhost:5173`). First boot compiles WASM and opens an `usaf_memo` live session — expect a few seconds on cold start.

Playwright e2e (builds WASM once, then drives the dev server):

```bash
cd crates/richtext-spikes/form-poc && npm install && npx playwright install chromium && npm run test:e2e
```

`form-poc` imports `editor-pm` source via a Vite alias; ProseMirror packages are pinned to `form-poc/node_modules` in `vite.config.js` so schema and view share one `prosemirror-model` instance.
