import { expect, test } from "@playwright/test";
import { deferred, withWorkspace } from "./helpers";

const cases = [
  { endpoint: "providers", outcome: "fails", hasFallback: true },
  { endpoint: "jobs/health", outcome: "fails", hasFallback: true },
  { endpoint: "providers", outcome: "stalls", hasFallback: true },
  { endpoint: "jobs/health", outcome: "stalls", hasFallback: true },
  { endpoint: "jobs/health", outcome: "stalls", hasFallback: false },
] as const;

function metadata(endpoint: string, stale: boolean) {
  return endpoint === "providers"
    ? {
        data: [
          {
            id: "test-provider",
            name: stale ? "Old provider" : "Current provider",
          },
        ],
      }
    : {
        data: {
          worker_last_seen: null,
          worker_healthy: true,
          queued: stale ? 999 : 42,
          uncertain: 0,
          manual_required: 0,
          oldest_due: null,
        },
      };
}

for (const { endpoint, outcome, hasFallback } of cases) {
  test(`archived brand clears when ${endpoint} ${outcome}, ${hasFallback ? "with" : "without"} a fallback`, async () => {
    await withWorkspace(async (page, context, write) => {
      const alpha = await write("/brands", { name: "Metadata Alpha" });
      const beta = hasFallback
        ? await write("/brands", { name: "Metadata Beta" })
        : null;
      for (const [brand, title] of [
        [alpha, "Alpha metadata draft"],
        ...(beta ? [[beta, "Beta metadata draft"]] : []),
      ]) {
        await write("/posts", {
          brand_id: brand.id,
          title,
          text: `${title} text`,
          media_ids: [],
          variants: [],
        });
      }
      await context.addInitScript(
        (id) => localStorage.setItem("beweft.brand", id),
        alpha.id,
      );
      const brandsRequested = deferred();
      const releaseBrands = deferred();
      const metadataRequested = deferred();
      const releaseMetadata = deferred();
      const metadataDelivered = deferred();
      let holdNextBrands = false;
      let interceptNextMetadata = false;
      const errors: string[] = [];
      page.on("pageerror", (error) => errors.push(error.message));
      await page.route("**/api/v1/brands", async (route) => {
        const response = await route.fetch();
        const envelope = await response.json();
        if (holdNextBrands) {
          holdNextBrands = false;
          brandsRequested.resolve();
          await releaseBrands.promise;
        }
        await route.fulfill({
          response,
          json: {
            data: envelope.data.filter(
              (brand: { id: string }) =>
                brand.id === alpha.id || brand.id === beta?.id,
            ),
          },
        });
      });
      await page.route(`**/api/v1/${endpoint}`, async (route) => {
        if (!interceptNextMetadata) {
          await route.fulfill({ json: metadata(endpoint, false) });
          return;
        }
        interceptNextMetadata = false;
        metadataRequested.resolve();
        if (outcome === "stalls") {
          await releaseMetadata.promise;
          await route.fulfill({ json: metadata(endpoint, true) });
        } else {
          await route.fulfill({
            status: 503,
            json: {
              error: {
                code: "fixture_failure",
                message: "Old metadata failed",
              },
            },
          });
        }
        metadataDelivered.resolve();
      });

      try {
        await page.clock.install();
        await page.goto("/#drafts");
        await expect(
          page.getByRole("heading", { name: "Alpha metadata draft" }),
        ).toBeVisible();
        await page.getByRole("button", { name: "Edit draft" }).click();
        await expect(page.getByLabel("Post text", { exact: true })).toHaveValue(
          "Alpha metadata draft text",
        );
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
        holdNextBrands = true;
        interceptNextMetadata = true;
        await page.clock.fastForward(30_000);
        await Promise.all([brandsRequested.promise, metadataRequested.promise]);
        if (outcome === "fails") {
          await metadataDelivered.promise;
          await page.evaluate(
            () =>
              new Promise<void>((resolve) =>
                requestAnimationFrame(() =>
                  requestAnimationFrame(() => resolve()),
                ),
              ),
          );
        }
        releaseBrands.resolve();

        await expect(page.getByRole("dialog")).toHaveCount(0);
        await expect(page.getByLabel("Current brand")).toHaveValue(
          beta?.id ?? "",
        );
        await expect(
          page.getByText("Alpha metadata draft", { exact: true }),
        ).toHaveCount(0);
        if (beta) {
          await expect(
            page.getByRole("heading", { name: "Beta metadata draft" }),
          ).toBeVisible();
          if (endpoint === "providers") {
            await page
              .getByRole("button", { name: "Connections", exact: true })
              .click();
            await expect(
              page.getByRole("button", { name: /Current provider/ }),
            ).toBeVisible();
          } else {
            await page
              .getByRole("button", { name: "Settings", exact: true })
              .click();
            await page
              .getByRole("button", { name: "System", exact: true })
              .click();
            await expect(
              page.locator("dt", { hasText: "Queued" }).locator("+ dd"),
            ).toHaveText("42");
          }
        } else {
          await expect(
            page.getByRole("heading", { name: "A fresh start for your ideas" }),
          ).toBeVisible();
        }
        if (outcome === "stalls") {
          releaseMetadata.resolve();
          await metadataDelivered.promise;
          await page.evaluate(
            () =>
              new Promise<void>((resolve) =>
                requestAnimationFrame(() =>
                  requestAnimationFrame(() => resolve()),
                ),
              ),
          );
          if (beta && endpoint === "providers") {
            await expect(
              page.getByRole("button", { name: /Current provider/ }),
            ).toBeVisible();
            await expect(
              page.getByRole("button", { name: /Old provider/ }),
            ).toHaveCount(0);
          } else if (beta) {
            await expect(
              page.locator("dt", { hasText: "Queued" }).locator("+ dd"),
            ).toHaveText("42");
          }
        }
        await expect(page.getByRole("alert")).toHaveCount(0);
        expect(errors).toEqual([]);
      } finally {
        releaseBrands.resolve();
        releaseMetadata.resolve();
      }
    });
  });
}

for (const endpoint of ["providers", "jobs/health"]) {
  test(`a late ${endpoint} response cannot overwrite a manually selected brand`, async () => {
    await withWorkspace(async (page, context, write) => {
      const alpha = await write("/brands", { name: "Metadata switch Alpha" });
      const beta = await write("/brands", { name: "Metadata switch Beta" });
      await write("/posts", {
        brand_id: beta.id,
        title: "Metadata switch Beta draft",
        text: "Isolated switch test",
        media_ids: [],
        variants: [],
      });
      await context.addInitScript(
        (id) => localStorage.setItem("beweft.brand", id),
        alpha.id,
      );
      const brandsDelivered = deferred();
      const metadataRequested = deferred();
      const releaseMetadata = deferred();
      const metadataDelivered = deferred();
      let observeNextBrands = false;
      let holdNextMetadata = false;
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
        if (observeNextBrands) {
          observeNextBrands = false;
          brandsDelivered.resolve();
        }
      });
      await page.route(`**/api/v1/${endpoint}`, async (route) => {
        if (!holdNextMetadata) {
          await route.fulfill({ json: metadata(endpoint, false) });
          return;
        }
        holdNextMetadata = false;
        metadataRequested.resolve();
        await releaseMetadata.promise;
        await route.fulfill({ json: metadata(endpoint, true) });
        metadataDelivered.resolve();
      });

      try {
        await page.clock.install();
        await page.goto("/#drafts");
        await expect(page.getByLabel("Current brand")).toHaveValue(alpha.id);
        observeNextBrands = true;
        holdNextMetadata = true;
        await page.clock.fastForward(30_000);
        await Promise.all([brandsDelivered.promise, metadataRequested.promise]);
        await page.evaluate(
          () =>
            new Promise<void>((resolve) =>
              requestAnimationFrame(() =>
                requestAnimationFrame(() => resolve()),
              ),
            ),
        );
        await page.getByLabel("Current brand").selectOption(beta.id);
        await expect(
          page.getByRole("heading", { name: "Metadata switch Beta draft" }),
        ).toBeVisible();
        if (endpoint === "providers") {
          await page
            .getByRole("button", { name: "Connections", exact: true })
            .click();
          await expect(
            page.getByRole("button", { name: /Current provider/ }),
          ).toBeVisible();
        } else {
          await page
            .getByRole("button", { name: "Settings", exact: true })
            .click();
          await page
            .getByRole("button", { name: "System", exact: true })
            .click();
          await expect(
            page.locator("dt", { hasText: "Queued" }).locator("+ dd"),
          ).toHaveText("42");
        }
        releaseMetadata.resolve();
        await metadataDelivered.promise;
        await page.evaluate(
          () =>
            new Promise<void>((resolve) =>
              requestAnimationFrame(() =>
                requestAnimationFrame(() => resolve()),
              ),
            ),
        );
        if (endpoint === "providers") {
          await expect(
            page.getByRole("button", { name: /Current provider/ }),
          ).toBeVisible();
          await expect(
            page.getByRole("button", { name: /Old provider/ }),
          ).toHaveCount(0);
        } else {
          await expect(
            page.locator("dt", { hasText: "Queued" }).locator("+ dd"),
          ).toHaveText("42");
        }
      } finally {
        releaseMetadata.resolve();
      }
    });
  });
}
