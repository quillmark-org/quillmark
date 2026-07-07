import { test as base, expect } from "@playwright/test";

/** One WASM boot per worker — cold `engine.open` is ~30–60s; don't repeat per test. */
export const test = base.extend({
  pocPage: [
    async ({ browser }, use) => {
      const page = await browser.newPage();
      /** @type {string[]} */
      const errors = [];
      page.on("pageerror", (err) => errors.push(String(err)));
      page.on("console", (msg) => {
        if (msg.type() === "error") errors.push(msg.text());
      });

      await page.goto("/");
      await page.waitForFunction(
        () => window.__POC__?.ready === true || window.__POC__?.error != null,
        { timeout: 120_000 }
      );
      const bootError = await page.evaluate(() => window.__POC__?.error ?? null);
      expect(bootError, "POC boot failed").toBeNull();

      await use(page);

      expect(errors, `unexpected page errors:\n${errors.join("\n")}`).toEqual([]);
      await page.close();
    },
    { scope: "worker" },
  ],
});

export { expect };
