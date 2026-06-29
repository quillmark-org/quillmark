// Type-level DRIFT GUARD for the canonical render types.
//
// `runtime/runtime.d.ts` defines the render-side types (`RenderResult`,
// `RenderOptions`, `Artifact`, `OutputFormat`, `PageSize`, `PaintOptions`,
// `PaintResult`) as the backend-NEUTRAL canonical contract, rather than
// re-exporting them from the private Typst backend build. This file asserts that
// those canonical declarations and the Typst backend's GENERATED declarations
// (`pkg/backends/typst/wasm.d.ts`, produced from the `typescript_custom_section`
// blocks in `crates/bindings/wasm/src/engine.rs`) stay mutually assignable — if
// either side drifts, one of the assignments below stops compiling.
//
// Run via `npm run typecheck` (tsc --noEmit). This file emits no runtime code.

import type {
	RenderResult as CanonicalRenderResult,
	RenderOptions as CanonicalRenderOptions,
	Artifact as CanonicalArtifact,
	OutputFormat as CanonicalOutputFormat,
	PageSize as CanonicalPageSize,
	PaintOptions as CanonicalPaintOptions,
	PaintResult as CanonicalPaintResult,
	FieldRegion as CanonicalFieldRegion
  // The BUILT copy (synced from `runtime/runtime.d.ts` by build-wasm.sh / the
  // cp step), because only there does the d.ts's own `../core/wasm.js` import
  // resolve to the generated `pkg/core` build. The two copies are byte-identical.
} from '../../../pkg/runtime/runtime.d.ts';

import type {
	RenderResult as TypstRenderResult,
	RenderOptions as TypstRenderOptions,
	Artifact as TypstArtifact,
	OutputFormat as TypstOutputFormat,
	PageSize as TypstPageSize,
	PaintOptions as TypstPaintOptions,
	PaintResult as TypstPaintResult,
	FieldRegion as TypstFieldRegion
} from '../../../pkg/backends/typst/wasm';

// One mutual-assignability pair per hoisted type: typst → canonical and
// canonical → typst. `void` the bindings so "declared but never read" is not an
// error under noUnusedLocals.

const renderResultA: CanonicalRenderResult = {} as TypstRenderResult;
const renderResultB: TypstRenderResult = {} as CanonicalRenderResult;
void renderResultA;
void renderResultB;

const renderOptionsA: CanonicalRenderOptions = {} as TypstRenderOptions;
const renderOptionsB: TypstRenderOptions = {} as CanonicalRenderOptions;
void renderOptionsA;
void renderOptionsB;

const artifactA: CanonicalArtifact = {} as TypstArtifact;
const artifactB: TypstArtifact = {} as CanonicalArtifact;
void artifactA;
void artifactB;

const outputFormatA: CanonicalOutputFormat = {} as TypstOutputFormat;
const outputFormatB: TypstOutputFormat = {} as CanonicalOutputFormat;
void outputFormatA;
void outputFormatB;

const pageSizeA: CanonicalPageSize = {} as TypstPageSize;
const pageSizeB: TypstPageSize = {} as CanonicalPageSize;
void pageSizeA;
void pageSizeB;

const paintOptionsA: CanonicalPaintOptions = {} as TypstPaintOptions;
const paintOptionsB: TypstPaintOptions = {} as CanonicalPaintOptions;
void paintOptionsA;
void paintOptionsB;

const paintResultA: CanonicalPaintResult = {} as TypstPaintResult;
const paintResultB: TypstPaintResult = {} as CanonicalPaintResult;
void paintResultA;
void paintResultB;

const fieldRegionA: CanonicalFieldRegion = {} as TypstFieldRegion;
const fieldRegionB: TypstFieldRegion = {} as CanonicalFieldRegion;
void fieldRegionA;
void fieldRegionB;
