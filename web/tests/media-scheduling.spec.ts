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

test("a corrupt known asset intent blocks only that asset and remains intact", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Corrupt media intent" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Corrupt intent channel",
      timezone: "UTC",
    });
    const damaged = await upload(context, brand.id, "damaged.png");
    const clear = await upload(context, brand.id, "clear.png");
    const damagedPost = await write("/posts", {
      brand_id: brand.id,
      title: "Damaged asset draft",
      text: "Fixture",
      media_ids: [damaged.id],
      variants: [{ channel_id: channel.id }],
    });
    await write("/posts", {
      brand_id: brand.id,
      title: "Clear asset draft",
      text: "Fixture",
      media_ids: [clear.id],
      variants: [{ channel_id: channel.id }],
    });
    const owner = (await (await context.request.get("/api/v1/auth/me")).json())
      .data;
    const key = `beweft.media-description:${owner.id}:${brand.id}:${damaged.id}`;
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.goto("/#drafts");
    const good = {
      id: crypto.randomUUID(),
      brand_id: brand.id,
      media_id: damaged.id,
      metadata_version: 1,
      alt_text: "Unknown outcome",
    };
    const corrupt = [
      "{invalid",
      "",
      JSON.stringify({ ...good, brand_id: crypto.randomUUID() }),
      JSON.stringify({ ...good, media_id: clear.id }),
      JSON.stringify({ ...good, metadata_version: "invalid" }),
    ];
    // No event is delivered after the dialog becomes ready. The final storage
    // scan must still prevent even a draft revision for the affected asset.
    await page
      .locator(".draft-content")
      .filter({ hasText: "Damaged asset draft" })
      .click();
    await page.getByLabel("When should this go out?").selectOption("exact");
    await page.getByLabel(/Date and time/).fill(future());
    const staleDialog = page.getByRole("dialog");
    await expect(
      staleDialog.getByRole("button", { name: "Schedule post" }),
    ).toBeEnabled();
    await page.evaluate(({ key, raw }) => localStorage.setItem(key, raw), {
      key,
      raw: corrupt[0],
    });
    await staleDialog.getByRole("button", { name: "Schedule post" }).click();
    await expect(staleDialog.getByRole("alert")).toContainText(
      "could not be read",
    );
    expect(await publications(context, brand.id)).toEqual([]);
    expect(
      (
        await (
          await context.request.get(`/api/v1/posts/${damagedPost.id}`)
        ).json()
      ).data.version,
    ).toBe(damagedPost.version);
    expect(await page.evaluate((key) => localStorage.getItem(key), key)).toBe(
      corrupt[0],
    );
    await staleDialog
      .getByRole("button", { name: "Cancel", exact: true })
      .click();
    for (const raw of corrupt) {
      await page.evaluate(
        ({ key, raw }) => {
          localStorage.setItem(key, raw);
          window.dispatchEvent(
            new CustomEvent("beweft-media-intent", { detail: key }),
          );
        },
        { key, raw },
      );
      await expect(
        page.getByText(/Some media descriptions could not be checked/),
      ).toBeVisible();
      await page
        .locator(".draft-content")
        .filter({ hasText: "Damaged asset draft" })
        .click();
      await page.getByLabel("When should this go out?").selectOption("exact");
      await page.getByLabel(/Date and time/).fill(future());
      const dialog = page.getByRole("dialog");
      await expect(
        dialog.getByText(/Pending description information could not be read/),
      ).toBeVisible();
      await expect(
        dialog.getByRole("button", { name: "Schedule post" }),
      ).toBeDisabled();
      await dialog.getByRole("button", { name: "Check media again" }).click();
      await expect(
        dialog.getByText(/Pending description information could not be read/),
      ).toBeVisible();
      await expect(
        dialog.getByRole("button", { name: "Schedule post" }),
      ).toBeDisabled();
      expect(await page.evaluate((key) => localStorage.getItem(key), key)).toBe(
        raw,
      );
      expect(
        (
          await (
            await context.request.get(`/api/v1/media/${damaged.id}`)
          ).json()
        ).data.metadata_version,
      ).toBe(1);
      expect(await publications(context, brand.id)).toEqual([]);
      await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
    }
    await page
      .locator(".draft-content")
      .filter({ hasText: "Clear asset draft" })
      .click();
    await page.getByLabel("When should this go out?").selectOption("exact");
    await page.getByLabel(/Date and time/).fill(future());
    await expect(
      page.getByRole("button", { name: "Schedule post" }),
    ).toBeEnabled();
    await page.getByRole("button", { name: "Schedule post" }).click();
    await expect
      .poll(async () => (await publications(context, brand.id)).length)
      .toBe(1);
    expect(
      (await publications(context, brand.id))[0].payload.media_metadata,
    ).toEqual([{ id: clear.id, alt_text: "" }]);
    expect(await page.evaluate((key) => localStorage.getItem(key), key)).toBe(
      corrupt.at(-1),
    );
  });
});

for (const failure of ["enumeration", "record read"] as const) {
  test(`unavailable media storage ${failure} blocks every schedule, including at execution time`, async () => {
    await withWorkspace(async (page, context, write) => {
      const brand = await write("/brands", {
        name: `Unavailable media ${failure}`,
      });
      const channel = await write("/channels", {
        brand_id: brand.id,
        provider: "manual",
        name: "Storage channel",
        timezone: "UTC",
      });
      const pending = await upload(context, brand.id, "storage-pending.png");
      const clear = await upload(context, brand.id, "storage-clear.png");
      const post = await write("/posts", {
        brand_id: brand.id,
        title: "Storage clear draft",
        text: "Fixture",
        media_ids: [clear.id],
        variants: [{ channel_id: channel.id }],
      });
      const owner = (
        await (await context.request.get("/api/v1/auth/me")).json()
      ).data;
      const key = `beweft.media-description:${owner.id}:${brand.id}:${pending.id}`;
      const raw = JSON.stringify({
        id: crypto.randomUUID(),
        brand_id: brand.id,
        media_id: pending.id,
        metadata_version: 1,
        alt_text: "Still pending",
      });
      await context.addInitScript(
        ({ id, key, raw, failure }) => {
          localStorage.setItem("beweft.brand", id);
          localStorage.setItem(key, raw);
          const originalKey = Storage.prototype.key;
          const originalGet = Storage.prototype.getItem;
          Storage.prototype.key = function (index) {
            if (
              this === localStorage &&
              (window as any).failMediaStorage &&
              failure === "enumeration"
            )
              throw new Error("Media storage unavailable");
            return originalKey.call(this, index);
          };
          Storage.prototype.getItem = function (name) {
            if (
              this === localStorage &&
              (window as any).failMediaStorage &&
              failure === "record read" &&
              name === key
            )
              throw new Error("Media storage unavailable");
            return originalGet.call(this, name);
          };
        },
        { id: brand.id, key, raw, failure },
      );
      await page.goto("/#drafts");
      await page
        .locator(".draft-content")
        .filter({ hasText: "Storage clear draft" })
        .click();
      await page.getByLabel("When should this go out?").selectOption("exact");
      await page.getByLabel(/Date and time/).fill(future());
      const dialog = page.getByRole("dialog");
      await expect(
        dialog.getByRole("button", { name: "Schedule post" }),
      ).toBeEnabled();
      // No storage event is delivered, so the execution-time scan must catch it.
      await page.evaluate(() => {
        (window as any).failMediaStorage = true;
      });
      await dialog.getByRole("button", { name: "Schedule post" }).click();
      await expect(dialog.getByRole("alert")).toContainText(
        "Media storage unavailable",
      );
      expect(await publications(context, brand.id)).toEqual([]);
      expect(
        (await (await context.request.get(`/api/v1/posts/${post.id}`)).json())
          .data.version,
      ).toBe(post.version);
      await page.evaluate(
        (key) =>
          window.dispatchEvent(
            new CustomEvent("beweft-media-intent", { detail: key }),
          ),
        key,
      );
      await expect(
        dialog.getByText(/must be checked before scheduling/),
      ).toBeVisible();
      await expect(
        dialog.getByRole("button", { name: "Schedule post" }),
      ).toBeDisabled();
      await page.evaluate(() => {
        (window as any).failMediaStorage = false;
      });
      await dialog.getByRole("button", { name: "Check media again" }).click();
      await expect(
        dialog.getByRole("button", { name: "Schedule post" }),
      ).toBeEnabled();
      expect(await page.evaluate((key) => localStorage.getItem(key), key)).toBe(
        raw,
      );
      await dialog.getByRole("button", { name: "Schedule post" }).click();
      await expect
        .poll(async () => (await publications(context, brand.id)).length)
        .toBe(1);
    });
  });
}

test("malformed current-scope keys block globally while other owners and brands stay isolated", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Unidentifiable media key" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Unknown key channel",
      timezone: "UTC",
    });
    const item = await upload(context, brand.id, "unknown-key.png");
    const post = await write("/posts", {
      brand_id: brand.id,
      title: "Unknown key draft",
      text: "Fixture",
      media_ids: [item.id],
      variants: [{ channel_id: channel.id }],
    });
    const owner = (await (await context.request.get("/api/v1/auth/me")).json())
      .data;
    const otherOwner = crypto.randomUUID();
    const otherBrand = crypto.randomUUID();
    const otherMedia = crypto.randomUUID();
    const scope = `beweft.media-description:${owner.id}:${brand.id}`;
    const foreign = [
      {
        key: `beweft.media-description:${otherOwner}:${brand.id}:${item.id}`,
        value: JSON.stringify({
          id: crypto.randomUUID(),
          brand_id: brand.id,
          media_id: item.id,
          metadata_version: 1,
          alt_text: "Another owner",
        }),
      },
      {
        key: `beweft.media-description:${owner.id}:${otherBrand}:${otherMedia}`,
        value: JSON.stringify({
          id: crypto.randomUUID(),
          brand_id: otherBrand,
          media_id: otherMedia,
          metadata_version: 1,
          alt_text: "Another brand",
        }),
      },
    ];
    await context.addInitScript(
      ({ id, foreign }) => {
        localStorage.setItem("beweft.brand", id);
        foreign.forEach(({ key, value }) => localStorage.setItem(key, value));
      },
      { id: brand.id, foreign },
    );
    await page.goto("/#drafts");
    await page.getByRole("button", { name: "Edit draft" }).click();
    await page.getByLabel("When should this go out?").selectOption("exact");
    await page.getByLabel(/Date and time/).fill(future());
    const dialog = page.getByRole("dialog");
    await expect(
      dialog.getByRole("button", { name: "Schedule post" }),
    ).toBeEnabled();
    // The storage event may be delayed. Submission must still detect this
    // namespace before creating a new draft revision.
    await page.evaluate(
      (scope) => localStorage.setItem(scope, "{invalid"),
      scope,
    );
    await dialog.getByRole("button", { name: "Schedule post" }).click();
    await expect(dialog.getByRole("alert")).toContainText(
      "could not be read safely",
    );
    expect(
      (await (await context.request.get(`/api/v1/posts/${post.id}`)).json())
        .data.version,
    ).toBe(post.version);
    expect(await publications(context, brand.id)).toEqual([]);
    await page.evaluate((scope) => localStorage.removeItem(scope), scope);
    const keys = [
      scope,
      `${scope}${item.id}`,
      `${scope}:`,
      `${scope}:not-a-canonical-id`,
      `${scope}::${item.id}`,
      `${scope}:${item.id}:extra`,
      `${scope.toUpperCase()}:${item.id}`,
    ];
    if (owner.id !== owner.id.toUpperCase())
      keys.push(
        `beweft.media-description:${owner.id.toUpperCase()}:${brand.id}:${item.id}`,
      );
    if (brand.id !== brand.id.toUpperCase())
      keys.push(
        `beweft.media-description:${owner.id}:${brand.id.toUpperCase()}:${item.id}`,
      );
    for (const key of keys) {
      await page.evaluate((key) => {
        localStorage.setItem(key, "{invalid");
        window.dispatchEvent(
          new CustomEvent("beweft-media-intent", { detail: key }),
        );
      }, key);
      await expect(
        dialog.getByText(/must be checked before scheduling/),
      ).toBeVisible();
      await expect(
        dialog.getByRole("button", { name: "Schedule post" }),
      ).toBeDisabled();
      await dialog.getByRole("button", { name: "Check media again" }).click();
      await expect(
        dialog.getByText(/must be checked before scheduling/),
      ).toBeVisible();
      expect(await page.evaluate((key) => localStorage.getItem(key), key)).toBe(
        "{invalid",
      );
      expect(await publications(context, brand.id)).toEqual([]);
      // Remove only the synthetic fixture so the next malformed spelling can be tested.
      await page.evaluate((key) => {
        localStorage.removeItem(key);
        window.dispatchEvent(
          new CustomEvent("beweft-media-intent", { detail: key }),
        );
      }, key);
      await expect(
        dialog.getByRole("button", { name: "Schedule post" }),
      ).toBeEnabled();
    }
    for (const { key, value } of foreign)
      expect(await page.evaluate((key) => localStorage.getItem(key), key)).toBe(
        value,
      );
    await dialog.getByRole("button", { name: "Schedule post" }).click();
    await expect
      .poll(async () => (await publications(context, brand.id)).length)
      .toBe(1);
  });
});

test("a delimiterless record keeps an in-flight media write blocked across tabs", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Held media namespace" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Namespace channel",
      timezone: "UTC",
    });
    const item = await upload(context, brand.id, "held-namespace.png");
    await write("/posts", {
      brand_id: brand.id,
      title: "Held namespace draft",
      text: "Fixture",
      media_ids: [item.id],
      variants: [{ channel_id: channel.id }],
    });
    const owner = (await (await context.request.get("/api/v1/auth/me")).json())
      .data;
    const scope = `beweft.media-description:${owner.id}:${brand.id}`;
    const key = `${scope}:${item.id}`;
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.goto("/#drafts");
    await page.getByRole("button", { name: "Edit draft" }).click();
    await page.getByLabel("When should this go out?").selectOption("exact");
    await page.getByLabel(/Date and time/).fill(future());
    const dialog = page.getByRole("dialog");
    await expect(
      dialog.getByRole("button", { name: "Schedule post" }),
    ).toBeEnabled();
    const writer = await context.newPage();
    const started = deferred(),
      release = deferred();
    await writer.route(`**/api/v1/media/${item.id}`, async (route) => {
      if (route.request().method() !== "PATCH") return route.continue();
      started.resolve();
      await release.promise;
      await route.continue().catch(() => {});
    });
    try {
      await writer.goto("/#media");
      await writer
        .locator(".media-grid")
        .getByRole("button", { name: /held-namespace\.png/ })
        .click();
      await writer
        .getByLabel("Description", { exact: true })
        .fill("The held description");
      await writer.getByRole("button", { name: "Save description" }).click();
      await started.promise;
      await expect(
        dialog.getByRole("button", { name: "Schedule post" }),
      ).toBeDisabled();
      const raw = await writer.evaluate(
        ({ key, scope }) => {
          const raw = localStorage.getItem(key);
          if (raw === null) throw new Error("Expected a pending intent");
          localStorage.setItem(scope, raw);
          localStorage.removeItem(key);
          return raw;
        },
        { key, scope },
      );
      await expect(
        dialog.getByText(/must be checked before scheduling/),
      ).toBeVisible();
      await expect(
        dialog.getByRole("button", { name: "Schedule post" }),
      ).toBeDisabled();
      const before = (
        await (await context.request.get(`/api/v1/media/${item.id}`)).json()
      ).data;
      expect({
        alt_text: before.alt_text,
        metadata_version: before.metadata_version,
      }).toEqual({ alt_text: "", metadata_version: 1 });
      expect(await publications(context, brand.id)).toEqual([]);
      expect(
        await page.evaluate((scope) => localStorage.getItem(scope), scope),
      ).toBe(raw);
      release.resolve();
      await expect
        .poll(
          async () =>
            (
              await (
                await context.request.get(`/api/v1/media/${item.id}`)
              ).json()
            ).data.metadata_version,
        )
        .toBe(2);
      await expect(
        dialog.getByRole("button", { name: "Schedule post" }),
      ).toBeDisabled();
      expect(
        await page.evaluate((scope) => localStorage.getItem(scope), scope),
      ).toBe(raw);
      expect(await publications(context, brand.id)).toEqual([]);
    } finally {
      release.resolve();
      await writer.close();
    }
  });
});

test("a failed refresh for another pending asset does not block unaffected media", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", {
      name: "Independent media recovery",
    });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Independent channel",
      timezone: "UTC",
    });
    const pending = await upload(context, brand.id, "pending-other.png");
    const clear = await upload(context, brand.id, "unaffected.png");
    await write("/posts", {
      brand_id: brand.id,
      title: "Pending asset draft",
      text: "Fixture",
      media_ids: [pending.id],
      variants: [{ channel_id: channel.id }],
    });
    await write("/posts", {
      brand_id: brand.id,
      title: "Unaffected asset draft",
      text: "Fixture",
      media_ids: [clear.id],
      variants: [{ channel_id: channel.id }],
    });
    const owner = (await (await context.request.get("/api/v1/auth/me")).json())
      .data;
    const key = `beweft.media-description:${owner.id}:${brand.id}:${pending.id}`;
    await context.addInitScript(
      ({ id, key, mediaId }) => {
        localStorage.setItem("beweft.brand", id);
        localStorage.setItem(
          key,
          JSON.stringify({
            id: crypto.randomUUID(),
            brand_id: id,
            media_id: mediaId,
            metadata_version: 1,
            alt_text: "Unconfirmed elsewhere",
          }),
        );
      },
      { id: brand.id, key, mediaId: pending.id },
    );
    await page.route(`**/api/v1/media/${pending.id}`, (route) =>
      route.request().method() === "GET"
        ? route.fulfill({
            status: 503,
            json: {
              error: { code: "unavailable", message: "Asset read unavailable" },
            },
          })
        : route.continue(),
    );
    await page.goto("/#drafts");
    await expect(
      page.getByText(/Some media descriptions could not be checked/),
    ).toBeVisible();
    await page
      .locator(".draft-content")
      .filter({ hasText: "Pending asset draft" })
      .click();
    await page.getByLabel("When should this go out?").selectOption("exact");
    await page.getByLabel(/Date and time/).fill(future());
    await expect(
      page.getByRole("button", { name: "Schedule post" }),
    ).toBeDisabled();
    await page.getByRole("button", { name: "Cancel", exact: true }).click();
    await page
      .locator(".draft-content")
      .filter({ hasText: "Unaffected asset draft" })
      .click();
    await page.getByLabel("When should this go out?").selectOption("exact");
    await page.getByLabel(/Date and time/).fill(future());
    await expect(
      page.getByRole("button", { name: "Schedule post" }),
    ).toBeEnabled();
    await page.getByRole("button", { name: "Schedule post" }).click();
    await expect
      .poll(async () => (await publications(context, brand.id)).length)
      .toBe(1);
    expect(
      (await publications(context, brand.id))[0].payload.media_metadata,
    ).toEqual([{ id: clear.id, alt_text: "" }]);
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
    const owner = (await (await context.request.get("/api/v1/auth/me")).json())
      .data;
    const key = `beweft.media-description:${owner.id}:${brand.id}:${item.id}`;
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
      await expect
        .poll(() =>
          other.evaluate(
            async (lockKey) =>
              (await navigator.locks.query()).held.some(
                (lock) => lock.name === lockKey,
              ),
            key,
          ),
        )
        .toBe(true);
      await other
        .locator(".media-grid")
        .getByRole("button", { name: /locked\.png/ })
        .click();
      await expect(other.getByRole("dialog").getByRole("alert")).toContainText(
        "Media is busy",
        { timeout: 15000 },
      );
      await expect(
        other.getByLabel("Description", { exact: true }),
      ).toBeDisabled();
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
      ).toHaveValue("Before scheduling");
      await other
        .getByLabel("Description", { exact: true })
        .fill("After scheduling");
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
