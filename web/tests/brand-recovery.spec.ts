import { expect, test } from "@playwright/test";
import { deferred, withWorkspace } from "./helpers";

test("archiving the active brand clears its draft and composer even if the fallback load fails", async () => {
  await withWorkspace(async (page, context, write) => {
    const alpha = await write("/brands", { name: "Recovery Alpha" });
    const beta = await write("/brands", { name: "Recovery Beta" });
    await write("/channels", {
      brand_id: alpha.id,
      provider: "manual",
      name: "Alpha-only channel",
      timezone: "UTC",
    });
    await write("/posts", {
      brand_id: alpha.id,
      title: "Alpha-only draft",
      text: "Alpha-only draft text",
      media_ids: [],
      variants: [],
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      alpha.id,
    );
    // The harness database is shared by scenarios; restrict the brand list to
    // these fixtures while keeping the archive and collection reads real.
    await page.route("**/api/v1/brands", async (route) => {
      const response = await route.fetch();
      const envelope = await response.json();
      await route.fulfill({
        response,
        json: {
          data: envelope.data.filter(
            (brand: { id: string }) =>
              brand.id === alpha.id || brand.id === beta.id,
          ),
        },
      });
    });

    const alphaRequested = deferred();
    const releaseAlpha = deferred();
    const alphaDelivered = deferred();
    const betaRequested = deferred();
    const releaseBeta = deferred();
    let holdAlpha = false;
    await page.route("**/api/v1/posts?*", async (route) => {
      const brandId = new URL(route.request().url()).searchParams.get(
        "brand_id",
      );
      if (brandId === alpha.id && holdAlpha) {
        const response = await route.fetch();
        alphaRequested.resolve();
        await releaseAlpha.promise;
        await route.fulfill({ response });
        alphaDelivered.resolve();
      } else if (brandId === beta.id) {
        betaRequested.resolve();
        await releaseBeta.promise;
        await route.fulfill({
          status: 503,
          json: {
            error: { code: "fixture_failure", message: "Beta load failed" },
          },
        });
      } else await route.continue();
    });

    try {
      await page.clock.install();
      await page.goto("/#drafts");
      await expect(
        page.getByRole("heading", { name: "Alpha-only draft" }),
      ).toBeVisible();
      await page.getByRole("button", { name: "Edit draft" }).click();
      await expect(page.getByLabel("Post text", { exact: true })).toHaveValue(
        "Alpha-only draft text",
      );

      // An older Alpha refresh has already fetched its response when the next
      // periodic refresh discovers the archive and starts loading Beta.
      holdAlpha = true;
      await page.clock.fastForward(30_000);
      await alphaRequested.promise;
      const archived = await context.request.patch(
        `/api/v1/brands/${alpha.id}`,
        {
          data: { archived: true },
          headers: {
            "X-Beweft-Request": "1",
            Origin: process.env.BEWEFT_TEST_URL!,
          },
        },
      );
      expect(archived.ok(), await archived.text()).toBe(true);
      await page.clock.fastForward(30_000);
      await betaRequested.promise;

      await expect(page.getByLabel("Current brand")).toHaveValue(beta.id);
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await expect(
        page.getByText("Alpha-only draft", { exact: true }),
      ).toHaveCount(0);
      await expect(
        page.getByRole("button", { name: "Edit draft" }),
      ).toHaveCount(0);

      releaseBeta.resolve();
      await expect(page.getByRole("alert")).toContainText("Beta load failed");
      releaseAlpha.resolve();
      await alphaDelivered.promise;
      await page.evaluate(
        () =>
          new Promise<void>((resolve) =>
            requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
          ),
      );
      await expect(page.getByLabel("Current brand")).toHaveValue(beta.id);
      await expect(
        page.getByText("Alpha-only draft", { exact: true }),
      ).toHaveCount(0);
      await expect(
        page.getByRole("button", { name: "Edit draft" }),
      ).toHaveCount(0);
      await page
        .getByRole("button", { name: "Create post", exact: true })
        .click();
      await expect(page.getByLabel("Working title")).toHaveValue("");
      await expect(page.getByLabel("Post text", { exact: true })).toHaveValue(
        "",
      );
      await expect(
        page.getByRole("button", { name: "Alpha-only channel" }),
      ).toHaveCount(0);
    } finally {
      releaseAlpha.resolve();
      releaseBeta.resolve();
    }
  });
});
