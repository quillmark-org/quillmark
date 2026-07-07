import { test, expect } from "./fixtures.js";

async function canvasIsPainted(page) {
  return page.evaluate(() => {
    const c = document.getElementById("page-canvas");
    if (!c || c.width < 100 || c.height < 100) return false;
    const ctx = c.getContext("2d");
    if (!ctx) return false;
    const { data } = ctx.getImageData(0, 0, c.width, c.height);
    for (let i = 0; i < data.length; i += 4) {
      const [r, g, b, a] = [data[i], data[i + 1], data[i + 2], data[i + 3]];
      if (a > 0 && (r < 250 || g < 250 || b < 250)) return true;
    }
    return false;
  });
}

async function clickFieldOnCanvas(page, field) {
  const point = await page.evaluate((f) => window.__POC__.regionClickPoint(f), field);
  expect(point, `region for ${field}`).toBeTruthy();
  await page.locator("#page-canvas").click({ position: point });
}

test.describe.configure({ mode: "serial" });

test.describe("richtext form POC", () => {
  test("boots with seeded richtext fields, paints canvas, exposes regions", async ({ pocPage }) => {
    await expect(pocPage.locator("#body-editor textarea")).toHaveValue(/first paragraph/);
    await expect(pocPage.locator("#subject-editor .ProseMirror")).toContainText("Richtext");
    await expect(pocPage.locator("#tag-line-editor .ProseMirror")).toContainText("Semper");
    await expect(pocPage.locator("#status")).toContainText("Ready");

    await expect.poll(() => canvasIsPainted(pocPage), { timeout: 15_000 }).toBe(true);

    await pocPage.locator("details.debug summary").click();
    const dump = pocPage.locator("#regions-dump");
    await expect(dump).toContainText('"field": "$body"');
    await expect(dump).toContainText('"field": "subject"');
    await expect(dump).toContainText('"field": "tag_line"');
  });

  test("body edit triggers live apply", async ({ pocPage }) => {
    const editor = pocPage.locator("#body-editor textarea");
    await editor.click();
    await editor.press("End");
    await editor.pressSequentially("!");
    await expect(pocPage.locator("#status")).toContainText("Applied ($body)", { timeout: 15_000 });
    await expect(editor).toHaveValue(/!$/);
  });

  test("subject edit triggers live apply", async ({ pocPage }) => {
    const editor = pocPage.locator("#subject-editor .ProseMirror");
    await editor.click();
    await editor.press("End");
    await editor.pressSequentially("!");
    await expect(pocPage.locator("#status")).toContainText("Applied (subject)", { timeout: 15_000 });
    await expect(editor).toContainText("!");
  });

  test("tag line typing and bold toolbar work", async ({ pocPage }) => {
    const editor = pocPage.locator("#tag-line-editor .ProseMirror");
    await editor.click();
    await editor.press("End");
    await editor.pressSequentially("!");
    await expect(pocPage.locator("#status")).toContainText("Applied (tag_line)", { timeout: 15_000 });
    await expect(editor).toContainText("!");

    await editor.click({ clickCount: 3 });
    await pocPage.locator("#tag-line-toolbar [data-mark='strong']").click();
    await expect(pocPage.locator("#status")).toContainText("Applied (tag_line)", { timeout: 15_000 });
  });

  test("region cross-navigation: form focus ↔ preview highlight ↔ canvas click", async ({ pocPage }) => {
    const highlight = pocPage.locator("#region-highlight");

    await pocPage.locator("#body-editor textarea").click();
    await expect(highlight).toHaveAttribute("data-field", "$body");

    await pocPage.locator("#tag-line-editor .ProseMirror").click();
    await expect(highlight).toHaveAttribute("data-field", "tag_line");

    await pocPage.locator("#subject-editor .ProseMirror").click();
    await expect(highlight).toHaveAttribute("data-field", "subject");

    await clickFieldOnCanvas(pocPage, "$body");
    await expect(pocPage.locator("#body-editor textarea")).toBeFocused();
    await expect(highlight).toHaveAttribute("data-field", "$body");

    await clickFieldOnCanvas(pocPage, "tag_line");
    await expect(pocPage.locator("#tag-line-editor .ProseMirror")).toBeFocused();
    await expect(highlight).toHaveAttribute("data-field", "tag_line");

    await clickFieldOnCanvas(pocPage, "subject");
    await expect(pocPage.locator("#subject-editor .ProseMirror")).toBeFocused();
    await expect(highlight).toHaveAttribute("data-field", "subject");
  });
});
