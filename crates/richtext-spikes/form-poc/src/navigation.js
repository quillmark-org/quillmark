/**
 * Canvas ↔ form cross-navigation using LiveSession fieldAt / positionAt.
 */

/**
 * Map a viewport click to PDF points (bottom-left origin).
 * @param {HTMLCanvasElement} canvas
 * @param {{ widthPt: number, heightPt: number }} pageSize
 * @param {number} clientX
 * @param {number} clientY
 */
export function canvasClickToPdfPoint(canvas, pageSize, clientX, clientY) {
  const rect = canvas.getBoundingClientRect();
  const relX = clientX - rect.left;
  const relY = clientY - rect.top;
  return {
    x: (relX / rect.width) * pageSize.widthPt,
    y: pageSize.heightPt - (relY / rect.height) * pageSize.heightPt,
  };
}

/**
 * Map a PDF-point (bottom-left) to canvas-local CSS pixels (top-left).
 * @param {HTMLCanvasElement} canvas
 * @param {{ widthPt: number, heightPt: number }} pageSize
 * @param {number} x
 * @param {number} y
 */
export function pdfPointToCanvasLocal(canvas, pageSize, x, y) {
  const rect = canvas.getBoundingClientRect();
  return {
    x: (x / pageSize.widthPt) * rect.width,
    y: (1 - y / pageSize.heightPt) * rect.height,
  };
}

/**
 * Center of a field region in canvas-local CSS pixels (for tests / synthetic clicks).
 * @param {HTMLCanvasElement} canvas
 * @param {{ widthPt: number, heightPt: number }} pageSize
 * @param {import('@quillmark/runtime').FieldRegion} region
 */
export function regionCenterOnCanvas(canvas, pageSize, region) {
  const [x0, y0, x1, y1] = region.rect;
  const cx = (x0 + x1) / 2;
  const cy = (y0 + y1) / 2;
  return pdfPointToCanvasLocal(canvas, pageSize, cx, cy);
}

/**
 * Wire preview canvas clicks → richtext field focus.
 * @param {HTMLCanvasElement} canvas
 * @param {() => import('@quillmark/runtime').LiveSession | null} getSession
 * @param {Record<string, { focus(): void, setCursor(usv: number): void }>} fields
 * @param {(field: string) => void} onActiveField
 */
export function wireCanvasNavigation(canvas, getSession, fields, onActiveField) {
  canvas.addEventListener("click", (ev) => {
    const session = getSession();
    if (!session) return;

    const pageSize = session.pageSize(0);
    const { x, y } = canvasClickToPdfPoint(canvas, pageSize, ev.clientX, ev.clientY);
    const hit = session.positionAt(0, x, y);
    const field = hit?.field ?? session.fieldAt(0, x, y);
    if (!field || !fields[field]) return;

    onActiveField(field);
    if (hit && typeof hit.pos === "number") {
      fields[field].setCursor(hit.pos);
    } else {
      fields[field].focus();
    }
  });
}
