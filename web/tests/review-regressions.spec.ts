import { test, expect } from "@playwright/test";
import { deferred, withWorkspace } from "./helpers";

test("brand changes isolate schedules, delayed mutations, and delayed analytics", async () => {
  await withWorkspace(async (page, context, write) => {
    const a = await write("/brands", { name: "Race brand A" });
    const b = await write("/brands", { name: "Race brand B" });
    const ca = await write("/channels", {
      brand_id: a.id,
      provider: "manual",
      name: "Channel A",
      timezone: "America/Chicago",
    });
    const cb = await write("/channels", {
      brand_id: b.id,
      provider: "manual",
      name: "Channel B",
      timezone: "Europe/London",
    });
    await write("/schedules", {
      channel_id: ca.id,
      days: [1],
      time: "09:00",
      timezone: ca.timezone,
    });
    await write("/schedules", {
      channel_id: cb.id,
      days: [2],
      time: "14:00",
      timezone: cb.timezone,
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      a.id,
    );
    const delayedSchedule = deferred(),
      requested = deferred(),
      scheduleDelivered = deferred();
    await page.route(`**/schedules?channel_id=${ca.id}`, async (route) => {
      const response = await route.fetch();
      requested.resolve();
      await delayedSchedule.promise;
      await route.fulfill({ response });
      scheduleDelivered.resolve();
    });
    await page.goto("/#settings");
    await requested.promise;
    await page.getByLabel("Current brand").selectOption(b.id);
    await expect(
      page.getByRole("combobox", { name: "Channel", exact: true }),
    ).toHaveValue(cb.id);
    await expect(page.getByLabel("Timezone", { exact: true })).toHaveValue(
      cb.timezone,
    );
    await expect(
      page.getByRole("button", { name: /Remove 14:00/ }),
    ).toBeVisible();
    delayedSchedule.resolve();
    await scheduleDelivered.promise;
    await page.evaluate(
      () =>
        new Promise<void>((resolve) =>
          requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
        ),
    );
    await expect(
      page.getByRole("button", { name: /Remove 09:00/ }),
    ).toHaveCount(0);
    await page.unroute(`**/schedules?channel_id=${ca.id}`);

    // A successful A mutation completes after B has loaded: it must not reload A.
    await page.getByLabel("Current brand").selectOption(a.id);
    await page
      .getByRole("button", { name: "Connections", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Channel A", exact: true }),
    ).toBeVisible();
    const mutation = deferred(),
      mutated = deferred();
    await page.route(`**/channels/${ca.id}`, async (route) => {
      const response = await route.fetch();
      mutated.resolve();
      await mutation.promise;
      await route.fulfill({ response });
    });
    await page.getByRole("button", { name: "Pause", exact: true }).click();
    await mutated.promise;
    await page.getByLabel("Current brand").selectOption(b.id);
    await expect(
      page.getByRole("heading", { name: "Channel B", exact: true }),
    ).toBeVisible();
    mutation.resolve();
    await expect(
      page.getByRole("button", { name: "Pause", exact: true }),
    ).toBeEnabled();
    await expect(page.getByLabel("Current brand")).toHaveValue(b.id);
    await expect(
      page.getByRole("heading", { name: "Channel A", exact: true }),
    ).toHaveCount(0);

    // Controlled read fixtures expose response ordering without provider access.
    const analytics = deferred(),
      analyticsRequested = deferred(),
      analyticsDelivered = deferred();
    const result = (reason: string) => ({
      data: {
        availability: "partial",
        reason,
        summary: { scheduled: 0, published: 0, attention: 0 },
        metrics: [],
      },
    });
    await page.route("**/analytics?*", async (route) => {
      if (route.request().url().includes(a.id)) {
        analyticsRequested.resolve();
        await analytics.promise;
        await route.fulfill({ json: result("Only brand A data") });
        analyticsDelivered.resolve();
      } else await route.fulfill({ json: result("Only brand B data") });
    });
    await page.getByLabel("Current brand").selectOption(a.id);
    await page.getByRole("button", { name: "Analytics", exact: true }).click();
    await analyticsRequested.promise;
    await page.getByLabel("Current brand").selectOption(b.id);
    await expect(page.getByText("Only brand B data")).toBeVisible();
    analytics.resolve();
    await analyticsDelivered.promise;
    await page.evaluate(
      () =>
        new Promise<void>((resolve) =>
          requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
        ),
    );
    await page.getByRole("button", { name: "Refresh workspace" }).click();
    await expect(page.getByText("Only brand A data")).toHaveCount(0);
    await expect(page.getByText("Only brand B data")).toBeVisible();
  });
});

test("channel-only drafts, filtered reorder, calendar date, and mobile focus use real workflows", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Composer regression" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Variant channel",
      timezone: "America/Chicago",
    });
    await write("/schedules", {
      channel_id: channel.id,
      days: [1, 2, 3, 4, 5, 6, 7],
      time: "09:00",
      timezone: channel.timezone,
    });
    const ids = [];
    for (const title of ["Visible first", "Hidden second", "Hidden third"]) {
      const post = await write("/posts", {
        brand_id: brand.id,
        title,
        text: "Isolated queue regression",
        media_ids: [],
        variants: [],
      });
      const publications = await write(`/posts/${post.id}/schedule`, {
        version: post.version,
        channel_ids: [channel.id],
      });
      ids.push(publications[0].id);
    }
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.goto("/#queue");
    await page.getByLabel("Search queue").fill("Visible first");
    const reordered = page.waitForRequest(
      (request) =>
        request.url().endsWith("/queue/reorder") && request.method() === "POST",
    );
    await page
      .getByRole("button", { name: "Move Visible first later" })
      .click();
    expect((await reordered).postDataJSON().publication_ids).toEqual([
      ids[1],
      ids[0],
      ids[2],
    ]);
    await expect(
      page.getByText("Queue reordered", { exact: true }),
    ).toBeVisible();
    await page
      .getByRole("button", { name: "Create post", exact: true })
      .click();
    await page.getByRole("button", { name: /Variant channel$/ }).click();
    await page.getByRole("tab", { name: "Variant channel" }).click();
    await page
      .getByLabel("Variant channel text")
      .fill("Only the channel version has text.");
    await page.getByRole("button", { name: "Save draft", exact: true }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    const posts = await context.request.get(
      `/api/v1/posts?brand_id=${brand.id}`,
    );
    expect(
      (await posts.json()).data.some(
        (p: any) =>
          p.text === "" &&
          p.variants.some(
            (v: any) => v.text === "Only the channel version has text.",
          ),
      ),
    ).toBe(true);
    await page.getByRole("button", { name: "Calendar", exact: true }).click();
    await page.getByRole("button", { name: "Next month" }).click();
    await page.locator(".calendar-day:not(.outside)").nth(14).click();
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Create post" })
      .click();
    await expect(page.getByLabel("When should this go out?")).toHaveValue(
      "exact",
    );
    expect(
      await page.locator('input[type="datetime-local"]').inputValue(),
    ).toMatch(/-15T09:00$/);
    await page.getByRole("button", { name: "Close dialog" }).click();
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(page.locator(".sidebar")).toHaveAttribute("inert", "");
    const trigger = page.getByRole("button", { name: "Open navigation" });
    await trigger.click();
    await expect(
      page.getByRole("dialog", { name: "Workspace navigation" }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Sign out" }).focus();
    await page.keyboard.press("Tab");
    expect(
      await page.evaluate(() => !!document.activeElement?.closest(".sidebar")),
    ).toBe(true);
    await page.keyboard.press("Shift+Tab");
    await expect(page.getByRole("button", { name: "Sign out" })).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(trigger).toBeFocused();
    await expect(page.locator(".sidebar")).toHaveAttribute("inert", "");
  });
});

test("partial provider sync errors and reconnect/disconnect are visible (controlled API fixtures)", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Provider UI fixtures" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Fixture connected account",
      timezone: "UTC",
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.route("**/channels?*", (route) =>
      route.fulfill({
        json: {
          data: [
            {
              ...channel,
              provider: "youtube",
              connection_status: "connected",
              capabilities: {
                permissions: {
                  source: "oauth",
                  reported_scopes: ["video.read"],
                  actions: {
                    publish: {
                      status: "missing",
                      required_scopes: ["video.write"],
                      missing_scopes: ["video.write"],
                    },
                  },
                },
              },
            },
          ],
        },
      }),
    );
    await page.route("**/providers", (route) =>
      route.fulfill({
        json: {
          data: [
            {
              id: "youtube",
              name: "YouTube",
              implementation: "available",
              configuration: "ready",
              execution: "direct",
              verification: "unverified",
            },
          ],
        },
      }),
    );
    await page.route(`**/channels/${channel.id}/sync`, (route) =>
      route.fulfill({
        json: {
          data: {
            metrics_observed: 2,
            inbox_items: 0,
            coverage: "partial_recent_page",
            errors: ["Provider permission is missing"],
            note: "Recent page only.",
          },
        },
      }),
    );
    await page.route(`**/channels/${channel.id}/disconnect`, (route) =>
      route.fulfill({
        json: {
          data: {
            disconnected: true,
            provider_revoked: false,
            next_action: "Revoke in provider account settings.",
          },
        },
      }),
    );
    await page.route("**/provider-apps", (route) =>
      route.fulfill({
        json: {
          data: [
            {
              provider: "youtube",
              client_id: "fixture",
              instance_url: null,
              configured: true,
            },
          ],
        },
      }),
    );
    await page.goto("/#analytics");
    await page.getByRole("button", { name: "Sync a channel" }).click();
    await page.getByRole("button", { name: "Sync now", exact: true }).click();
    await expect(page.getByRole("status")).toContainText(
      "Synchronization incomplete: 2 metrics observed, 0 inbox items",
    );
    await expect(page.getByRole("status")).toContainText(
      "Provider permission is missing",
    );
    await page
      .getByRole("button", { name: "Connections", exact: true })
      .click();
    await expect(
      page.getByText("Connection: connected", { exact: true }),
    ).toBeVisible();
    await expect(
      page.getByText("Queue running", { exact: true }),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Reconnect", exact: true }),
    ).toBeEnabled();
    await page.getByText("Account permissions", { exact: true }).click();
    await expect(
      page.getByText("missing · Reconnect for: video.write", { exact: true }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Reconnect", exact: true }).click();
    await expect(
      page.getByRole("heading", { name: "Reconnect YouTube", exact: true }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Close dialog" }).click();
    await page.getByRole("button", { name: "Disconnect", exact: true }).click();
    await page
      .getByRole("button", { name: "Disconnect channel", exact: true })
      .click();
    await expect(
      page.getByText("Revoke in provider account settings."),
    ).toBeVisible();
  });
});
