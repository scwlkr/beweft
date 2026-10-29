import { expect, test } from "@playwright/test";
import { deferred, withWorkspace } from "./helpers";

test("creating the first brand closes the dialog and selects the brand", async () => {
  await withWorkspace(async (page, context) => {
    const name = `First brand ${crypto.randomUUID()}`;
    await context.addInitScript(() => localStorage.removeItem("beweft.brand"));
    await page.route("**/api/v1/brands", async (route) => {
      if (route.request().method() !== "GET") return route.continue();
      const response = await route.fetch();
      const envelope = await response.json();
      await route.fulfill({
        response,
        json: {
          data: envelope.data.filter(
            (brand: { name: string }) => brand.name === name,
          ),
        },
      });
    });
    await page.goto("/#drafts");
    await expect(
      page.getByRole("heading", { name: "A fresh start for your ideas" }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Create your first brand" }).click();
    await page.getByLabel("Brand name").fill(name);
    await page
      .getByRole("button", { name: "Create brand", exact: true })
      .click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await expect(
      page.getByLabel("Current brand").locator("option:checked"),
    ).toHaveText(name);
    const response = await context.request.get("/api/v1/brands");
    const rows = (await response.json()).data.filter(
      (brand: { name: string }) => brand.name === name,
    );
    expect(rows).toHaveLength(1);
    await page.unrouteAll({ behavior: "wait" });
  });
});

test("a completed brand creation closes without overriding a later manual choice", async () => {
  await withWorkspace(async (page, context, write) => {
    const alpha = await write("/brands", { name: "Create choice Alpha" });
    const beta = await write("/brands", { name: "Create choice Beta" });
    const name = `Created after choice ${crypto.randomUUID()}`;
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      alpha.id,
    );
    await page.goto("/#drafts");
    await expect(page.getByLabel("Current brand")).toHaveValue(alpha.id);
    const committed = deferred(),
      release = deferred();
    await page.route("**/api/v1/brands", async (route) => {
      if (route.request().method() !== "POST") return route.continue();
      const response = await route.fetch();
      committed.resolve();
      await release.promise;
      await route.fulfill({ response });
    });
    try {
      await page
        .getByRole("button", { name: "Create brand", exact: true })
        .click();
      await page.getByLabel("Brand name").fill(name);
      await page
        .getByRole("dialog")
        .getByRole("button", { name: "Create brand", exact: true })
        .click();
      await committed.promise;
      await page.locator("#brand-select").evaluate((element, id) => {
        const select = element as HTMLSelectElement;
        select.value = id;
        select.dispatchEvent(new Event("change", { bubbles: true }));
      }, beta.id);
      await expect(page.getByLabel("Current brand")).toHaveValue(beta.id);
      release.resolve();
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await expect(page.getByLabel("Current brand")).toHaveValue(beta.id);
      const response = await context.request.get("/api/v1/brands");
      expect(
        (await response.json()).data.filter(
          (brand: { name: string }) => brand.name === name,
        ),
      ).toHaveLength(1);
      await page.unrouteAll({ behavior: "wait" });
    } finally {
      release.resolve();
    }
  });
});
