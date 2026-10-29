import { expect, test, type BrowserContext, type Page } from "@playwright/test";
import { deferred, withWorkspace } from "./helpers";

type Write = (path: string, body: unknown) => Promise<any>;

async function fixtures(page: Page, context: BrowserContext, write: Write) {
  const alpha = await write("/brands", { name: "Concurrency Alpha" });
  const beta = await write("/brands", { name: "Concurrency Beta" });
  for (const [brand, title] of [
    [alpha, "Alpha private draft"],
    [beta, "Beta independent draft"],
  ]) {
    await write("/posts", {
      brand_id: brand.id,
      title,
      text: `${title} text`,
      media_ids: [],
      variants: [],
    });
  }
  const allowed = new Set([alpha.id, beta.id]);
  let beforeBrandResponse: () => Promise<"fail" | void> = async () => {};
  await page.route("**/api/v1/brands", async (route) => {
    const response = await route.fetch();
    const envelope = await response.json();
    if ((await beforeBrandResponse()) === "fail") {
      await route.fulfill({
        status: 503,
        json: {
          error: { code: "fixture_failure", message: "Brand list unavailable" },
        },
      });
      return;
    }
    await route.fulfill({
      response,
      json: {
        data: envelope.data.filter((brand: { id: string }) =>
          allowed.has(brand.id),
        ),
      },
    });
  });
  await context.addInitScript(
    (id) => localStorage.setItem("beweft.brand", id),
    alpha.id,
  );
  await page.clock.install();
  await page.goto("/#drafts");
  await expect(
    page.getByRole("heading", { name: "Alpha private draft" }),
  ).toBeVisible();
  return {
    alpha,
    beta,
    allowed,
    beforeBrandResponse: (hook: () => Promise<"fail" | void>) => {
      beforeBrandResponse = hook;
    },
  };
}

async function archive(context: BrowserContext, id: string) {
  const response = await context.request.patch(`/api/v1/brands/${id}`, {
    data: { archived: true },
    headers: { "X-Beweft-Request": "1", Origin: process.env.BEWEFT_TEST_URL! },
  });
  expect(response.ok(), await response.text()).toBe(true);
}

async function afterPaint(page: Page) {
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  );
}

for (const newer of ["hangs", "fails"]) {
  test(`a completed archive discovery applies while a newer brand-list request ${newer}`, async () => {
    await withWorkspace(async (page, context, write) => {
      const { alpha, beta, beforeBrandResponse } = await fixtures(
        page,
        context,
        write,
      );
      await page.getByRole("button", { name: "Edit draft" }).click();
      await archive(context, alpha.id);
      const firstRequested = deferred(),
        secondRequested = deferred();
      const firstRelease = deferred(),
        secondRelease = deferred();
      let requests = 0;
      beforeBrandResponse(async () => {
        requests++;
        if (requests === 1) {
          firstRequested.resolve();
          await firstRelease.promise;
        } else if (requests === 2) {
          secondRequested.resolve();
          if (newer === "fails") return "fail";
          await secondRelease.promise;
        }
      });
      try {
        await page.clock.fastForward(30_000);
        await firstRequested.promise;
        const failure =
          newer === "fails"
            ? page.waitForResponse(
                (response) =>
                  response.url().endsWith("/api/v1/brands") &&
                  response.status() === 503,
              )
            : null;
        await page.clock.fastForward(30_000);
        await secondRequested.promise;
        if (failure) await failure;
        firstRelease.resolve();
        await expect(page.getByRole("dialog")).toHaveCount(0);
        await expect(page.getByLabel("Current brand")).toHaveValue(beta.id);
        await expect(
          page.getByRole("heading", { name: "Beta independent draft" }),
        ).toBeVisible();
        await expect(
          page.getByText("Alpha private draft", { exact: true }),
        ).toHaveCount(0);
      } finally {
        firstRelease.resolve();
        secondRelease.resolve();
      }
    });
  });
}

test("a newer successful list corrects an automatic fallback while its follow-up load is held", async () => {
  await withWorkspace(async (page, context, write) => {
    const { alpha, beta, allowed, beforeBrandResponse } = await fixtures(
      page,
      context,
      write,
    );
    const gamma = await write("/brands", { name: "Recovery Gamma" });
    await write("/posts", {
      brand_id: gamma.id,
      title: "Gamma replacement draft",
      text: "Gamma content",
      media_ids: [],
      variants: [],
    });
    allowed.add(gamma.id);
    for (const endpoint of ["providers", "jobs/health"]) {
      await page.route(`**/api/v1/${endpoint}`, (route) =>
        route.fulfill({
          status: 503,
          json: {
            error: { code: "unavailable", message: "Metadata unavailable" },
          },
        }),
      );
    }
    const oldRequested = deferred(),
      newRequested = deferred(),
      followupRequested = deferred();
    const releaseOld = deferred(),
      releaseNew = deferred(),
      releaseFollowup = deferred();
    let count = 0;
    beforeBrandResponse(async () => {
      count++;
      if (count === 1) {
        oldRequested.resolve();
        await releaseOld.promise;
      } else if (count === 2) {
        newRequested.resolve();
        await releaseNew.promise;
      } else if (count === 3) {
        followupRequested.resolve();
        await releaseFollowup.promise;
      }
    });
    try {
      await page.getByRole("button", { name: "Edit draft" }).click();
      await archive(context, alpha.id);
      await page.clock.fastForward(30_000);
      await oldRequested.promise; // Snapshot includes Beta.
      await archive(context, beta.id);
      await page.clock.fastForward(30_000);
      await newRequested.promise; // Newer snapshot excludes Beta.
      releaseOld.resolve();
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await expect(page.getByLabel("Current brand")).toHaveValue(beta.id);
      await followupRequested.promise;
      releaseNew.resolve();
      await expect(page.getByLabel("Current brand")).toHaveValue(gamma.id);
      await expect(
        page.getByRole("heading", { name: "Gamma replacement draft" }),
      ).toBeVisible();
      await expect(
        page.getByText("Alpha private draft", { exact: true }),
      ).toHaveCount(0);
      await page.getByRole("button", { name: "Edit draft" }).click();
      await expect(page.getByLabel("Post text", { exact: true })).toHaveValue(
        "Gamma content",
      );
    } finally {
      releaseOld.resolve();
      releaseNew.resolve();
      releaseFollowup.resolve();
    }
  });
});

test("a pending brand list cannot override a later manual selection", async () => {
  await withWorkspace(async (page, context, write) => {
    const { alpha, beta } = await fixtures(page, context, write);
    const oldRequested = deferred(),
      releaseOld = deferred();
    const nextRequested = deferred(),
      releaseNext = deferred();
    let count = 0;
    await page.route("**/api/v1/brands", async (route) => {
      const response = await route.fetch();
      const data = (await response.json()).data.filter(
        (brand: { id: string }) => [alpha.id, beta.id].includes(brand.id),
      );
      count++;
      if (count === 1) {
        oldRequested.resolve();
        await releaseOld.promise;
        await route.fulfill({
          response,
          json: {
            data: data.filter((brand: { id: string }) => brand.id !== beta.id),
          },
        });
      } else {
        nextRequested.resolve();
        await releaseNext.promise;
        await route.fulfill({ response, json: { data } });
      }
    });
    try {
      await page.clock.fastForward(30_000);
      await oldRequested.promise;
      await page.getByLabel("Current brand").selectOption(beta.id);
      await nextRequested.promise;
      releaseOld.resolve();
      await afterPaint(page);
      await expect(page.getByLabel("Current brand")).toHaveValue(beta.id);
      releaseNext.resolve();
      await expect(
        page.getByRole("heading", { name: "Beta independent draft" }),
      ).toBeVisible();
    } finally {
      releaseOld.resolve();
      releaseNext.resolve();
    }
  });
});

test("an older brand-list success cannot replace a newer applied list", async () => {
  await withWorkspace(async (page, context, write) => {
    const { allowed, beforeBrandResponse } = await fixtures(
      page,
      context,
      write,
    );
    const oldRequested = deferred(),
      oldRelease = deferred();
    let requests = 0;
    beforeBrandResponse(async () => {
      if (++requests === 1) {
        oldRequested.resolve();
        await oldRelease.promise;
      }
    });
    try {
      await page.clock.fastForward(30_000);
      await oldRequested.promise;
      const gamma = await write("/brands", { name: "Ordering Gamma" });
      allowed.add(gamma.id);
      await page.clock.fastForward(30_000);
      await expect(
        page.getByRole("option", { name: "Ordering Gamma" }),
      ).toHaveCount(1);
      const delivered = page.waitForResponse(
        (response) =>
          response.url().endsWith("/api/v1/brands") &&
          response.status() === 200,
      );
      oldRelease.resolve();
      await delivered;
      await afterPaint(page);
      await expect(
        page.getByRole("option", { name: "Ordering Gamma" }),
      ).toHaveCount(1);
    } finally {
      oldRelease.resolve();
    }
  });
});

for (const stalled of ["providers", "jobs/health"]) {
  test(`brand data and actions load during persistent ${stalled} outage`, async () => {
    await withWorkspace(async (page, context, write) => {
      const { alpha, beta } = await fixtures(page, context, write);
      const release = deferred(),
        multipleStalled = deferred();
      let enabled = false,
        stalledCount = 0;
      for (const endpoint of ["providers", "jobs/health"]) {
        await page.route(`**/api/v1/${endpoint}`, async (route) => {
          if (!enabled) return await route.continue();
          if (endpoint === stalled) {
            if (++stalledCount >= 2) multipleStalled.resolve();
            await release.promise;
            await route.continue();
          } else {
            await route.fulfill({
              status: 503,
              json: {
                error: {
                  code: "fixture_failure",
                  message: "Metadata unavailable",
                },
              },
            });
          }
        });
      }
      try {
        await page.getByRole("button", { name: "Edit draft" }).click();
        await archive(context, alpha.id);
        enabled = true;
        await page.clock.fastForward(30_000);
        await expect(page.getByRole("dialog")).toHaveCount(0);
        await expect(page.getByLabel("Current brand")).toHaveValue(beta.id);
        await expect(
          page.getByRole("heading", { name: "Beta independent draft" }),
        ).toBeVisible();
        await multipleStalled.promise;
        await page
          .getByRole("button", { name: "Delete Beta independent draft" })
          .click();
        await page
          .getByRole("button", { name: "Delete draft", exact: true })
          .click();
        await expect(page.getByRole("dialog")).toHaveCount(0);
        await expect(
          page.getByText("Draft deleted", { exact: true }),
        ).toBeVisible();
        await expect(
          page.getByRole("heading", { name: "Beta independent draft" }),
        ).toHaveCount(0);
      } finally {
        release.resolve();
      }
    });
  });
}

test("metadata 401 signs out while brand loading hangs and clears private state across sign-in", async () => {
  await withWorkspace(async (page, context, write) => {
    const { beforeBrandResponse } = await fixtures(page, context, write);
    const firstRequested = deferred(),
      secondRequested = deferred();
    const firstRelease = deferred(),
      secondRelease = deferred();
    let requests = 0,
      unauthorized = true;
    beforeBrandResponse(async () => {
      requests++;
      if (requests === 1) {
        firstRequested.resolve();
        await firstRelease.promise;
      } else if (requests === 2) {
        secondRequested.resolve();
        await secondRelease.promise;
      }
    });
    await page.route("**/api/v1/jobs/health", async (route) => {
      if (!unauthorized) return await route.continue();
      unauthorized = false;
      await firstRequested.promise;
      await route.fulfill({
        status: 401,
        json: { error: { code: "unauthorized", message: "Session expired" } },
      });
    });
    try {
      await page.getByRole("button", { name: "Edit draft" }).click();
      await page.clock.fastForward(30_000);
      await expect(page.getByLabel("Email address")).toBeVisible();
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await page
        .getByLabel("Email address")
        .fill(process.env.BEWEFT_TEST_EMAIL!);
      await page
        .getByLabel("Password", { exact: true })
        .fill(process.env.BEWEFT_TEST_PASSWORD!);
      await page.getByRole("button", { name: "Sign in", exact: true }).click();
      await secondRequested.promise;
      await expect(
        page.getByText("Alpha private draft", { exact: true }),
      ).toHaveCount(0);
      await expect(page.getByRole("dialog")).toHaveCount(0);
      const oldResponse = page.waitForResponse(
        (response) =>
          response.url().endsWith("/api/v1/brands") &&
          response.status() === 200,
      );
      firstRelease.resolve();
      await oldResponse;
      await afterPaint(page);
      await expect(
        page.getByText("Alpha private draft", { exact: true }),
      ).toHaveCount(0);
      secondRelease.resolve();
      await expect(
        page.getByRole("heading", { name: "Alpha private draft" }),
      ).toBeVisible();
      await expect(page.getByRole("dialog")).toHaveCount(0);
    } finally {
      firstRelease.resolve();
      secondRelease.resolve();
    }
  });
});

test("a 401 from the previous brand signs out the same session after a switch", async () => {
  await withWorkspace(async (page, context, write) => {
    const { beta } = await fixtures(page, context, write);
    const oldBrandsRequested = deferred(),
      release401 = deferred(),
      betaBrandsRequested = deferred(),
      releaseBeta = deferred();
    let requests = 0;
    await page.route("**/api/v1/brands", async (route) => {
      if (++requests === 1) {
        oldBrandsRequested.resolve();
        await release401.promise;
        await route.fulfill({
          status: 401,
          json: { error: { code: "unauthorized", message: "Session expired" } },
        });
      } else {
        betaBrandsRequested.resolve();
        await releaseBeta.promise;
        await route.continue();
      }
    });
    try {
      await page.clock.fastForward(30_000);
      await oldBrandsRequested.promise;
      await page.getByLabel("Current brand").selectOption(beta.id);
      await betaBrandsRequested.promise;
      release401.resolve();
      await expect(page.getByLabel("Email address")).toBeVisible();
      await expect(
        page.getByText("Alpha private draft", { exact: true }),
      ).toHaveCount(0);
    } finally {
      release401.resolve();
      releaseBeta.resolve();
    }
  });
});

test("a delayed 401 from the prior session cannot sign out a new session", async () => {
  await withWorkspace(async (page, context, write) => {
    await fixtures(page, context, write);
    const oldProviderRequested = deferred(),
      releaseOld401 = deferred(),
      old401Delivered = deferred();
    let interceptProvider = true,
      interceptHealth = true;
    await page.route("**/api/v1/providers", async (route) => {
      if (!interceptProvider) return await route.continue();
      interceptProvider = false;
      oldProviderRequested.resolve();
      await releaseOld401.promise;
      await route.fulfill({
        status: 401,
        json: {
          error: { code: "unauthorized", message: "Old session expired" },
        },
      });
      old401Delivered.resolve();
    });
    await page.route("**/api/v1/jobs/health", async (route) => {
      if (!interceptHealth) return await route.continue();
      interceptHealth = false;
      await oldProviderRequested.promise;
      await route.fulfill({
        status: 401,
        json: { error: { code: "unauthorized", message: "Session expired" } },
      });
    });
    try {
      await page.clock.fastForward(30_000);
      await expect(page.getByLabel("Email address")).toBeVisible();
      await page
        .getByLabel("Email address")
        .fill(process.env.BEWEFT_TEST_EMAIL!);
      await page
        .getByLabel("Password", { exact: true })
        .fill(process.env.BEWEFT_TEST_PASSWORD!);
      await page.getByRole("button", { name: "Sign in", exact: true }).click();
      await expect(
        page.getByRole("heading", { name: "Alpha private draft" }),
      ).toBeVisible();
      releaseOld401.resolve();
      await old401Delivered.promise;
      await afterPaint(page);
      await expect(
        page.getByRole("heading", { name: "Alpha private draft" }),
      ).toBeVisible();
      await expect(page.getByLabel("Email address")).toHaveCount(0);
    } finally {
      releaseOld401.resolve();
    }
  });
});
