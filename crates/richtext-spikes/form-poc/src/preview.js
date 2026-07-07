/**
 * Canvas paint + region overlay helpers (PDF pt, bottom-left origin).
 */

/**
 * @param {import('@quillmark/runtime').LiveSession} session
 * @param {number} page
 * @param {HTMLCanvasElement} canvas
 * @param {{ densityScale?: number, maxCssWidth?: number }} [opts]
 */
export function paintPage(session, page, canvas, opts = {}) {
  const densityScale = opts.densityScale ?? window.devicePixelRatio ?? 1;
  const pageSize = session.pageSize(page);
  const cssTarget =
    opts.maxCssWidth ??
    (canvas.parentElement?.clientWidth
      ? Math.min(canvas.parentElement.clientWidth - 48, 720)
      : 612);
  const layoutScale = cssTarget / pageSize.widthPt;

  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("2d context unavailable");

  // Painter owns canvas.width/height — do not assign them after paint (clears ink).
  const result = session.paint(ctx, page, { layoutScale, densityScale });
  canvas.style.width = `${result.layoutWidth}px`;
  canvas.style.height = `${result.layoutHeight}px`;
  return { ...result, layoutScale, pageSize };
}

/**
 * Position a highlight box over a field region.
 * @param {HTMLElement} el
 * @param {import('@quillmark/runtime').FieldRegion} region
 * @param {{ widthPt: number, heightPt: number }} pageSize
 * @param {HTMLElement} anchor — preview pane (position:relative)
 * @param {string} [field] — active field name (data attribute for tests)
 */
export function positionRegionHighlight(el, region, pageSize, anchor, field) {
  const canvas = anchor.querySelector("canvas");
  if (!canvas) return;
  const [x0, y0, x1, y1] = region.rect;
  const anchorRect = anchor.getBoundingClientRect();
  const canvasRect = canvas.getBoundingClientRect();

  const leftPct = (x0 / pageSize.widthPt) * 100;
  const topPct = (1 - y1 / pageSize.heightPt) * 100;
  const widthPct = ((x1 - x0) / pageSize.widthPt) * 100;
  const heightPct = ((y1 - y0) / pageSize.heightPt) * 100;

  el.hidden = false;
  if (field) el.dataset.field = field;
  el.style.left = `${canvasRect.left - anchorRect.left + (leftPct / 100) * canvasRect.width}px`;
  el.style.top = `${canvasRect.top - anchorRect.top + (topPct / 100) * canvasRect.height}px`;
  el.style.width = `${(widthPct / 100) * canvasRect.width}px`;
  el.style.height = `${(heightPct / 100) * canvasRect.height}px`;
}

/**
 * @param {import('@quillmark/runtime').FieldRegion[]} regions
 * @param {string} field
 */
export function findFieldRegion(regions, field) {
  return regions.find((r) => r.field === field && r.page === 0);
}
