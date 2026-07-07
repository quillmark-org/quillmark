/**
 * Load the usaf_memo fixture as a Quill.fromTree Map. Every file is fetched
 * via Vite asset URLs (text and binary share one path).
 */
const fileUrls = import.meta.glob(
  [
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/**",
    "!**/LICENSE",
    "!**/LICENSE.md",
    "!**/LICENSE.txt",
    "!**/*LICENSE*",
  ],
  { query: "?url", import: "default" }
);

/** @returns {Promise<Map<string, Uint8Array>>} */
export async function loadUsafMemoTree() {
  /** @type {Map<string, Uint8Array>} */
  const tree = new Map();
  const marker = "usaf_memo/0.2.0/";

  await Promise.all(
    Object.entries(fileUrls).map(async ([vitePath, urlLoader]) => {
      const idx = vitePath.indexOf(marker);
      if (idx < 0) return;
      const rel = vitePath.slice(idx + marker.length);
      const url = await urlLoader();
      const bytes = new Uint8Array(await (await fetch(url)).arrayBuffer());
      tree.set(rel, bytes);
    })
  );

  if (!tree.has("Quill.yaml")) {
    throw new Error("usaf_memo fixture incomplete — Quill.yaml missing from glob");
  }
  return tree;
}
