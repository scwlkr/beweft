import { expect, test, type BrowserContext } from "@playwright/test";
import { deferred, withWorkspace } from "./helpers";

const pixel = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aL1sAAAAASUVORK5CYII=",
  "base64",
);
async function upload(context: BrowserContext, brand: string, name: string) {
  const response = await context.request.post(
    `/api/v1/media?brand_id=${brand}`,
    {
      multipart: { file: { name, mimeType: "image/png", buffer: pixel } },
      headers: {
        "X-Beweft-Request": "1",
        Origin: process.env.BEWEFT_TEST_URL!,
      },
    },
  );
  expect(response.ok(), await response.text()).toBe(true);
  return (await response.json()).data;
}
async function publications(context: BrowserContext, brand: string) {
  const response = await context.request.get(
    `/api/v1/publications?brand_id=${brand}`,
  );
  expect(response.ok()).toBe(true);
  return (await response.json()).data;
}
const future = () =>
  new Date(Date.now() + 7 * 86_400_000).toISOString().slice(0, 16);

test("an unresolved asset in a nonactive selected variant blocks scheduling through reload and recovers in the composer", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Variant media recovery" });
    const first = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "First variant",
      timezone: "UTC",
    });
    const second = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Second variant",
      timezone: "UTC",
    });
    const item = await upload(context, brand.id, "variant.png");
    await write("/posts", {
      brand_id: brand.id,
      title: "Variant fixture",
      text: "Fixture text",
      media_ids: [],
      variants: [
        { channel_id: first.id },
        { channel_id: second.id, media_ids: [item.id] },
      ],
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    const committed = deferred(),
      release = deferred();
    let schedules = 0;
    page.on("request", (request) => {
      if (request.method() === "POST" && request.url().endsWith("/schedule"))
        schedules++;
    });
    await page.route(`**/api/v1/media/${item.id}`, async (route) => {
      if (route.request().method() !== "PATCH") return route.continue();
      const response = await route.fetch();
      committed.resolve();
      await release.promise;
      await route.fulfill({ response }).catch(() => {});
    });
    try {
      await page.goto("/#media");
      await page
        .locator(".media-grid")
        .getByRole("button", { name: /variant\.png/ })
        .click();
      await page
        .getByLabel("Description", { exact: true })
        .fill("Saved description in second variant");
      await page.getByRole("button", { name: "Save description" }).click();
      await committed.promise;
      await page.getByRole("button", { name: "Close dialog" }).click();
      await page
        .getByRole("button", { name: /^Drafts/ })
        .first()
        .click();
      await page.getByRole("button", { name: "Edit draft" }).click();
      await page.getByLabel("When should this go out?").selectOption("exact");
      await page.getByLabel(/Date and time/).fill(future());
      await expect(
        page.getByRole("tab", { name: "Shared version" }),
      ).toHaveAttribute("aria-selected", "true");
      await expect(
        page.getByRole("button", { name: "Schedule post" }),
      ).toBeDisabled();
      await expect(
        page
          .getByRole("dialog")
          .getByText(/selected media description is unconfirmed/),
      ).toBeVisible();
      const delayed = page.waitForResponse(
        (response) =>
          response.request().method() === "PATCH" &&
          response.url().endsWith(`/api/v1/media/${item.id}`),
      );
      release.resolve();
      await (await delayed).finished();
      await page.getByRole("tab", { name: "Second variant" }).click();
      await expect(
        page.getByText("variant.png: Saved description in second variant"),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Schedule post" }),
      ).toBeDisabled();
      await page.getByRole("button", { name: "Cancel", exact: true }).click();
      await page.reload();
      await page.getByRole("button", { name: "Edit draft" }).click();
      await page.getByLabel("When should this go out?").selectOption("exact");
      await page.getByLabel(/Date and time/).fill(future());
      await expect(
        page.getByRole("button", { name: "Schedule post" }),
      ).toBeDisabled();
      expect(schedules).toBe(0);
      expect(await publications(context, brand.id)).toEqual([]);
      await page
        .getByRole("dialog")
        .getByRole("button", { name: "Review variant.png" })
        .click();
      const recovery = page.getByRole("dialog", { name: "variant.png" });
      await expect(
        recovery.getByLabel("Description", { exact: true }),
      ).toHaveValue("Saved description in second variant");
      await expect(
        recovery.getByLabel("Description", { exact: true }),
      ).toBeEnabled();
      await recovery.getByRole("button", { name: "Close dialog" }).click();
      await page.getByRole("tab", { name: "Second variant" }).click();
      await expect(
        page.getByText("variant.png: Saved description in second variant"),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Schedule post" }),
      ).toBeEnabled();
      await page.getByRole("button", { name: "Schedule post" }).click();
      await expect
        .poll(async () => (await publications(context, brand.id)).length)
        .toBe(2);
      const rows = await publications(context, brand.id);
      expect(
        rows.find((row: { channel_id: string }) => row.channel_id === second.id)
          .payload.media_metadata,
      ).toEqual([
        { id: item.id, alt_text: "Saved description in second variant" },
      ]);
      expect(schedules).toBe(1);
    } finally {
      release.resolve();
    }
  });
});

test("a delayed storage event cannot bypass the execution-time guard or schedule a stale description", async () => {
  await withWorkspace(async (old, context, write) => {
    await old.addInitScript(() =>
      window.addEventListener(
        "storage",
        (event) => {
          if (event.key?.startsWith("beweft.media-description:"))
            event.stopImmediatePropagation();
        },
        true,
      ),
    );
    const brand = await write("/brands", { name: "Delayed media event" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Guarded channel",
      timezone: "UTC",
    });
    const item = await upload(context, brand.id, "guard.png");
    const post = await write("/posts", {
      brand_id: brand.id,
      title: "Guarded draft",
      text: "Fixture text",
      media_ids: [item.id],
      variants: [{ channel_id: channel.id }],
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await old.goto("/#drafts");
    await old.getByRole("button", { name: "Edit draft" }).click();
    await old.getByLabel("When should this go out?").selectOption("exact");
    await old.getByLabel(/Date and time/).fill(future());
    await expect(
      old.getByRole("button", { name: "Schedule post" }),
    ).toBeEnabled();
    const other = await context.newPage();
    const committed = deferred(),
      release = deferred();
    await other.route(`**/api/v1/media/${item.id}`, async (route) => {
      if (route.request().method() !== "PATCH") return route.continue();
      const response = await route.fetch();
      committed.resolve();
      await release.promise;
      await route.fulfill({ response }).catch(() => {});
    });
    try {
      await other.goto("/#media");
      await other
        .locator(".media-grid")
        .getByRole("button", { name: /guard\.png/ })
        .click();
      await other
        .getByLabel("Description", { exact: true })
        .fill("Changed in the other tab");
      await other.getByRole("button", { name: "Save description" }).click();
      await committed.promise;
      await expect(
        old.getByRole("button", { name: "Schedule post" }),
      ).toBeEnabled();
      await old.getByRole("button", { name: "Schedule post" }).click();
      await expect(old.getByRole("dialog").getByRole("alert")).toContainText(
        "still unconfirmed",
      );
      expect(await publications(context, brand.id)).toEqual([]);
      expect(
        (await (await context.request.get(`/api/v1/posts/${post.id}`)).json())
          .data.version,
      ).toBe(post.version);
      await other.getByRole("button", { name: "Check description" }).click();
      await expect(
        other.getByLabel("Description", { exact: true }),
      ).toBeEnabled();
      await old.getByRole("button", { name: "Schedule post" }).click();
      await expect(old.getByRole("dialog").getByRole("alert")).toContainText(
        "Media descriptions changed",
      );
      await expect(
        old.getByText("guard.png: Changed in the other tab"),
      ).toBeVisible();
      expect(await publications(context, brand.id)).toEqual([]);
      await old.getByRole("button", { name: "Schedule post" }).click();
      await expect
        .poll(async () => (await publications(context, brand.id)).length)
        .toBe(1);
      expect(
        (await publications(context, brand.id))[0].payload.media_metadata,
      ).toEqual([{ id: item.id, alt_text: "Changed in the other tab" }]);
    } finally {
      release.resolve();
      await other.close();
    }
  });
});

test("unreadable pending-media storage blocks scheduling until it can be checked", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Unreadable media intent" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Unreadable channel",
      timezone: "UTC",
    });
    const item = await upload(context, brand.id, "unreadable.png");
    await write("/posts", {
      brand_id: brand.id,
      title: "Unreadable draft",
      text: "Fixture",
      media_ids: [item.id],
      variants: [{ channel_id: channel.id }],
    });
    const owner = (await (await context.request.get("/api/v1/auth/me")).json())
      .data;
    const key = `beweft.media-description:${owner.id}:${brand.id}:${item.id}`;
    await context.addInitScript(
      ({ id, key }) => {
        localStorage.setItem("beweft.brand", id);
        localStorage.setItem(key, "{invalid");
      },
      { id: brand.id, key },
    );
    await page.goto("/#drafts");
    await page.getByRole("button", { name: "Edit draft" }).click();
    await page.getByLabel("When should this go out?").selectOption("exact");
    await page.getByLabel(/Date and time/).fill(future());
    await expect(
      page.getByRole("button", { name: "Schedule post" }),
    ).toBeDisabled();
    await expect(
      page.getByRole("dialog").getByText(/must be checked before scheduling/),
    ).toBeVisible();
    expect(await publications(context, brand.id)).toEqual([]);
    await page.evaluate(
      (storageKey) => localStorage.removeItem(storageKey),
      key,
    );
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Check media again" })
      .click();
    await expect(
      page.getByRole("button", { name: "Schedule post" }),
    ).toBeEnabled();
  });
});

test("an active schedule holds the asset guard without stranding a later media edit", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Schedule media lock" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Locked channel",
      timezone: "UTC",
    });
    const item = await upload(context, brand.id, "locked.png");
    const edit = await context.request.patch(`/api/v1/media/${item.id}`, {
      data: { metadata_version: 1, alt_text: "Before scheduling" },
      headers: {
        "X-Beweft-Request": "1",
        Origin: process.env.BEWEFT_TEST_URL!,
      },
    });
    expect(edit.ok()).toBe(true);
    const post = await write("/posts", {
      brand_id: brand.id,
      title: "Locked draft",
      text: "Fixture",
      media_ids: [item.id],
      variants: [{ channel_id: channel.id }],
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    const committed = deferred(),
      release = deferred();
    await page.route(`**/api/v1/posts/${post.id}/schedule`, async (route) => {
      const response = await route.fetch();
      committed.resolve();
      await release.promise;
      await route.fulfill({ response }).catch(() => {});
    });
    const other = await context.newPage();
    try {
      await page.goto("/#drafts");
      await page.getByRole("button", { name: "Edit draft" }).click();
      await page.getByLabel("When should this go out?").selectOption("exact");
      await page.getByLabel(/Date and time/).fill(future());
      await page.getByRole("button", { name: "Schedule post" }).click();
      await committed.promise;
      await other.goto("/#media");
      await other
        .locator(".media-grid")
        .getByRole("button", { name: /locked\.png/ })
        .click();
      await other
        .getByLabel("Description", { exact: true })
        .fill("After scheduling");
      await other.getByRole("button", { name: "Save description" }).click();
      await expect(other.getByRole("dialog").getByRole("alert")).toContainText(
        "Media is busy",
      );
      expect(
        (await (await context.request.get(`/api/v1/media/${item.id}`)).json())
          .data.metadata_version,
      ).toBe(2);
      release.resolve();
      await other.getByRole("button", { name: "Check description" }).click();
      await expect(
        other.getByLabel("Description", { exact: true }),
      ).toBeEnabled();
      await expect(
        other.getByLabel("Description", { exact: true }),
      ).toHaveValue("After scheduling");
      await other.getByRole("button", { name: "Save description" }).click();
      await expect(other.getByRole("dialog").getByRole("status")).toHaveText(
        "Description saved.",
      );
      expect(
        (await publications(context, brand.id))[0].payload.media_metadata,
      ).toEqual([{ id: item.id, alt_text: "Before scheduling" }]);
    } finally {
      release.resolve();
      await other.close();
    }
  });
});

test("NUL input is rejected before persistence, and an old invalid intent needs the specific rejection before correction", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Invalid media description" });
    const item = await upload(context, brand.id, "nul.png");
    const owner = (await (await context.request.get("/api/v1/auth/me")).json())
      .data;
    const key = `beweft.media-description:${owner.id}:${brand.id}:${item.id}`;
    const response = await context.request.patch(`/api/v1/media/${item.id}`, {
      data: { alt_text: "Before\0After", metadata_version: 1 },
      headers: {
        "X-Beweft-Request": "1",
        Origin: process.env.BEWEFT_TEST_URL!,
      },
    });
    expect(response.status()).toBe(400);
    expect((await response.json()).error.code).toBe(
      "media_description_contains_nul",
    );
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.goto("/#media");
    await page
      .locator(".media-grid")
      .getByRole("button", { name: /nul\.png/ })
      .click();
    await page.getByLabel("Description", { exact: true }).fill("Before\0After");
    await expect(
      page.getByText(/Descriptions cannot contain NUL characters/),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Save description" }),
    ).toBeDisabled();
    expect(
      await page.evaluate(
        (storageKey) => localStorage.getItem(storageKey),
        key,
      ),
    ).toBeNull();
    await page.getByRole("button", { name: "Close dialog" }).click();
    await page.evaluate(
      ({ storageKey, brandId, mediaId }) => {
        localStorage.setItem(
          storageKey,
          JSON.stringify({
            id: crypto.randomUUID(),
            brand_id: brandId,
            media_id: mediaId,
            metadata_version: 1,
            alt_text: "Before\0After",
          }),
        );
        window.dispatchEvent(
          new CustomEvent("beweft-media-intent", { detail: storageKey }),
        );
      },
      { storageKey: key, brandId: brand.id, mediaId: item.id },
    );
    await page
      .locator(".media-grid")
      .getByRole("button", { name: /nul\.png/ })
      .click();
    await expect(
      page.getByRole("button", { name: "Check invalid description" }),
    ).toBeVisible();
    const fail = (route: import("@playwright/test").Route) =>
      route.request().method() === "PATCH"
        ? route.fulfill({
            status: 500,
            json: {
              error: { code: "internal_error", message: "Temporary failure" },
            },
          })
        : route.continue();
    await page.route(`**/api/v1/media/${item.id}`, fail);
    await page
      .getByRole("button", { name: "Check invalid description" })
      .click();
    await expect(
      page
        .getByRole("dialog")
        .getByRole("alert")
        .filter({ hasText: "previous description remains unconfirmed" }),
    ).toBeVisible();
    expect(
      await page.evaluate(
        (storageKey) => localStorage.getItem(storageKey),
        key,
      ),
    ).not.toBeNull();
    await page.unroute(`**/api/v1/media/${item.id}`, fail);
    const unrelated = (route: import("@playwright/test").Route) =>
      route.request().method() === "PATCH"
        ? route.fulfill({
            status: 400,
            json: {
              error: {
                code: "invalid_request",
                message: "Different validation failure",
              },
            },
          })
        : route.continue();
    await page.route(`**/api/v1/media/${item.id}`, unrelated);
    await page
      .getByRole("button", { name: "Check invalid description" })
      .click();
    await expect(
      page
        .getByRole("dialog")
        .getByRole("alert")
        .filter({ hasText: "Different validation failure" }),
    ).toBeVisible();
    expect(
      await page.evaluate(
        (storageKey) => localStorage.getItem(storageKey),
        key,
      ),
    ).not.toBeNull();
    await page.unroute(`**/api/v1/media/${item.id}`, unrelated);
    await page
      .getByRole("button", { name: "Check invalid description" })
      .click();
    await expect(page.getByLabel("Description", { exact: true })).toBeEnabled();
    expect(
      await page.evaluate(
        (storageKey) => localStorage.getItem(storageKey),
        key,
      ),
    ).toBeNull();
    const before = (
      await (await context.request.get(`/api/v1/media/${item.id}`)).json()
    ).data;
    expect(before).toMatchObject({ metadata_version: 1, alt_text: "" });
    await page
      .getByLabel("Description", { exact: true })
      .fill("Before and after");
    await page.getByRole("button", { name: "Save description" }).click();
    await expect(page.getByRole("dialog").getByRole("status")).toHaveText(
      "Description saved.",
    );
    const after = (
      await (await context.request.get(`/api/v1/media/${item.id}`)).json()
    ).data;
    expect(after).toMatchObject({
      metadata_version: 2,
      alt_text: "Before and after",
    });
  });
});
