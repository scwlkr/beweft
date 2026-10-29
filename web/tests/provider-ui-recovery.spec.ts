import { expect, test } from "@playwright/test";
import { deferred, withWorkspace } from "./helpers";

async function setupConnection(
  page: any,
  context: any,
  write: any,
  provider = "bluesky",
) {
  const brand = await write("/brands", { name: "Connection recovery fixture" });
  const channel = await write("/channels", {
    brand_id: brand.id,
    provider: "manual",
    name: "Recovery account",
    timezone: "UTC",
  });
  const direct = { ...channel, provider, connection_status: "connected" };
  await context.addInitScript(
    (id: string) => localStorage.setItem("beweft.brand", id),
    brand.id,
  );
  await page.route("**/channels?*", (route: any) =>
    route.fulfill({ json: { data: [direct] } }),
  );
  await page.route("**/providers", (route: any) =>
    route.fulfill({
      json: {
        data: [
          {
            id: provider,
            name: provider === "bluesky" ? "Bluesky" : "YouTube",
            implementation: "available",
            configuration: "ready",
            execution: "direct",
          },
        ],
      },
    }),
  );
  await page.route("**/provider-apps", (route: any) =>
    route.fulfill({ json: { data: [] } }),
  );
  return { brand, channel, direct };
}

test("disconnect warns first and displays the server's removal instructions", async () => {
  await withWorkspace(async (page, context, write) => {
    const { channel } = await setupConnection(page, context, write);
    let bodies: unknown[] = [];
    await page.route(`**/channels/${channel.id}/disconnect`, (route) => {
      bodies.push(route.request().postDataJSON());
      return route.fulfill({
        json: {
          data: {
            disconnected: true,
            provider_revoked: false,
            channels_requiring_reconnection: 2,
            revocation_id: crypto.randomUUID(),
            next_action:
              "Inspect the provider's connected apps, then remove access.",
          },
        },
      });
    });
    await page.goto("/#connections");
    await page.getByRole("button", { name: "Disconnect", exact: true }).click();
    const dialog = page.getByRole("dialog");
    await expect(dialog).toContainText("cancels its queued deliveries");
    await expect(dialog).toContainText("accounts in other brands");
    await expect(dialog).toContainText("does not revoke access");
    await expect(
      dialog.getByRole("button", { name: "Disconnect channel" }),
    ).toBeDisabled();
    await dialog
      .getByRole("checkbox", { name: /I understand the selected queue/ })
      .check();
    await dialog.getByRole("button", { name: "Disconnect channel" }).click();
    await expect(dialog).toContainText(
      "Inspect the provider's connected apps, then remove access.",
    );
    await expect(dialog).toContainText(
      "2 other channel(s) were marked for reconnection",
    );
    expect(bodies).toEqual([{ confirm_shared_impact: true }]);
  });
});

test("lost disconnect response and stale removal ID require fresh review before reconnecting", async () => {
  await withWorkspace(async (page, context, write) => {
    const { brand, channel, direct } = await setupConnection(
      page,
      context,
      write,
    );
    const first = crypto.randomUUID(),
      second = crypto.randomUUID();
    let current: string | null = first;
    const acknowledgements: any[] = [],
      connections: any[] = [];
    await page.route(`**/channels/${channel.id}/disconnect`, (route) => {
      expect(route.request().postDataJSON()).toEqual({
        confirm_shared_impact: true,
      });
      return route.abort("failed");
    });
    await page.route("**/connections/bluesky/removal-status", (route) =>
      route.fulfill({
        json: { data: { provider: "bluesky", revocation_id: current } },
      }),
    );
    await page.route(
      "**/connections/bluesky/acknowledge-revocation",
      (route) => {
        const body = route.request().postDataJSON();
        acknowledgements.push(body);
        if (acknowledgements.length === 1) {
          current = second;
          return route.fulfill({
            status: 409,
            json: {
              error: {
                code: "revocation_id_conflict",
                message: "The removal changed.",
              },
            },
          });
        }
        current = null;
        return route.fulfill({
          json: { data: { revocation_acknowledged: true } },
        });
      },
    );
    await page.route("**/connections/bluesky", (route) => {
      connections.push(route.request().postDataJSON());
      return route.fulfill({ json: { data: direct } });
    });
    await page.goto("/#connections");
    await page.getByRole("button", { name: "Disconnect", exact: true }).click();
    await page
      .getByRole("checkbox", { name: /I understand the selected queue/ })
      .check();
    await page.getByRole("button", { name: "Disconnect channel" }).click();
    await expect(page.getByRole("dialog")).toContainText(
      "disconnect outcome may be unknown",
    );
    await page
      .getByRole("button", { name: "Review provider removal status" })
      .click();
    const dialog = page.getByRole("dialog");
    await expect(dialog).toContainText("This is your attestation");
    await expect(
      dialog.getByRole("button", { name: "Reconnect account" }),
    ).toBeDisabled();
    await dialog
      .getByRole("checkbox", { name: /I removed beweft access/ })
      .check();
    await dialog
      .getByRole("button", { name: "Confirm provider access removed" })
      .click();
    await expect(dialog).toContainText("The removal changed");
    expect(acknowledgements).toEqual([
      { access_removed: true, revocation_id: first },
    ]);
    await expect(
      dialog.getByRole("button", { name: "Reconnect account" }),
    ).toBeDisabled();
    await dialog.getByRole("button", { name: "Check removal status" }).click();
    await expect(
      dialog.getByRole("checkbox", { name: /I removed beweft access/ }),
    ).not.toBeChecked();
    await expect(
      dialog.getByRole("button", { name: "Confirm provider access removed" }),
    ).toBeDisabled();
    await dialog
      .getByRole("checkbox", { name: /I removed beweft access/ })
      .check();
    await dialog
      .getByRole("button", { name: "Confirm provider access removed" })
      .click();
    await expect(dialog).toContainText("Your removal attestation was recorded");
    expect(acknowledgements[1]).toEqual({
      access_removed: true,
      revocation_id: second,
    });
    await dialog.getByLabel("Bluesky identifier").fill("fixture.example");
    await dialog.getByLabel("App password").fill("fixture-only");
    await dialog.getByRole("button", { name: "Reconnect account" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    expect(connections).toHaveLength(1);
    expect(connections[0]).toMatchObject({
      brand_id: brand.id,
      reconnect_channel_id: channel.id,
    });
  });
});

test("a late provider authorization cannot redirect after the session changes", async () => {
  await withWorkspace(async (page, context, write) => {
    const { brand } = await setupConnection(page, context, write, "youtube");
    await page.route("**/connections/youtube/removal-status", (route) =>
      route.fulfill({
        json: { data: { provider: "youtube", revocation_id: null } },
      }),
    );
    await page.route("**/provider-apps", (route) =>
      route.fulfill({
        json: {
          data: [
            {
              provider: "youtube",
              client_id: "fixture",
              configured: true,
              instance_url: null,
            },
          ],
        },
      }),
    );
    const started = deferred(),
      release = deferred(),
      delivered = deferred();
    await page.route("**/connections/youtube/start", async (route) => {
      started.resolve();
      await release.promise;
      await route.fulfill({
        json: {
          data: {
            authorization_url: "https://example.invalid/authorize",
            flow_id: crypto.randomUUID(),
          },
        },
      });
      delivered.resolve();
    });
    const other = await context.newPage();
    try {
      await page.goto("/#connections");
      await other.goto("/#connections");
      await expect(other.getByLabel("Current brand")).toHaveValue(brand.id);
      await page
        .getByRole("button", { name: "Reconnect", exact: true })
        .click();
      await page.getByRole("button", { name: "Reconnect account" }).click();
      await started.promise;
      await other.getByRole("button", { name: "Sign out" }).click();
      await expect(
        other.getByRole("button", { name: "Sign in", exact: true }),
      ).toBeVisible();
      release.resolve();
      await delivered.promise;
      await expect(page).not.toHaveURL(/example\.invalid/);
      await expect(page.getByRole("dialog")).toHaveCount(0);
    } finally {
      release.resolve();
      await other.close();
    }
  });
});

test("a late authorization cannot redirect after the selected brand is archived", async () => {
  await withWorkspace(async (page, context, write) => {
    const { brand, direct } = await setupConnection(
      page,
      context,
      write,
      "youtube",
    );
    const fallback = await write("/brands", {
      name: "Connection fallback fixture",
    });
    await page.route("**/api/v1/brands", async (route) => {
      const response = await route.fetch(),
        body = await response.json();
      return route.fulfill({
        response,
        json: {
          data: body.data.filter((item: { id: string }) =>
            [brand.id, fallback.id].includes(item.id),
          ),
        },
      });
    });
    await page.route("**/channels?*", (route) => {
      const requestedBrand = new URL(route.request().url()).searchParams.get(
        "brand_id",
      );
      return route.fulfill({
        json: { data: requestedBrand === brand.id ? [direct] : [] },
      });
    });
    await page.route("**/connections/youtube/removal-status", (route) =>
      route.fulfill({
        json: { data: { provider: "youtube", revocation_id: null } },
      }),
    );
    await page.route("**/provider-apps", (route) =>
      route.fulfill({
        json: {
          data: [
            {
              provider: "youtube",
              client_id: "fixture",
              configured: true,
              instance_url: null,
            },
          ],
        },
      }),
    );
    const started = deferred(),
      release = deferred(),
      delivered = deferred();
    await page.route("**/connections/youtube/start", async (route) => {
      started.resolve();
      await release.promise;
      await route.fulfill({
        json: {
          data: {
            authorization_url: "https://example.invalid/authorize",
            flow_id: crypto.randomUUID(),
          },
        },
      });
      delivered.resolve();
    });
    try {
      await page.clock.install();
      await page.goto("/#connections");
      await page
        .getByRole("button", { name: "Reconnect", exact: true })
        .click();
      await page.getByRole("button", { name: "Reconnect account" }).click();
      await started.promise;
      const archived = await context.request.patch(
        `/api/v1/brands/${brand.id}`,
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
      await expect(page.getByLabel("Current brand")).toHaveValue(fallback.id);
      await expect(page.getByRole("dialog")).toHaveCount(0);
      release.resolve();
      await delivered.promise;
      await expect(page).not.toHaveURL(/example\.invalid/);
      await expect(page.getByLabel("Current brand")).toHaveValue(fallback.id);
    } finally {
      release.resolve();
    }
  });
});

test("destination flows reject a different brand and clear on a later brand change", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "OAuth flow brand" });
    const fallback = await write("/brands", { name: "OAuth flow fallback" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Flow fixture",
      timezone: "UTC",
    });
    const flowId = crypto.randomUUID();
    await context.addInitScript(
      (id: string) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    let archivedBrand = false;
    await page.route("**/api/v1/brands", (route) =>
      route.fulfill({
        json: { data: archivedBrand ? [fallback] : [brand, fallback] },
      }),
    );
    let flowBrand = fallback.id,
      posts = 0;
    const started = deferred(),
      release = deferred(),
      delivered = deferred();
    await page.route(`**/connection-flows/${flowId}`, (route) =>
      route.fulfill({
        json: {
          data: {
            id: flowId,
            brand_id: flowBrand,
            provider: "youtube",
            destinations: [
              { id: "fixture", name: "Fixture destination", username: null },
            ],
          },
        },
      }),
    );
    await page.route(
      `**/connection-flows/${flowId}/complete`,
      async (route) => {
        posts++;
        started.resolve();
        await release.promise;
        await route.fulfill({ json: { data: channel } });
        delivered.resolve();
      },
    );
    try {
      await page.clock.install();
      await page.goto(`/?connection_flow=${flowId}#connections`);
      await expect(page.getByRole("dialog")).toContainText(
        "belongs to a different brand or flow",
      );
      await expect(
        page.getByRole("button", { name: "Finish connection" }),
      ).toBeDisabled();
      expect(posts).toBe(0);
      flowBrand = brand.id;
      await page.reload();
      await page
        .getByRole("dialog")
        .getByRole("combobox")
        .selectOption("fixture");
      await page.getByRole("button", { name: "Finish connection" }).click();
      await started.promise;
      const archived = await context.request.patch(
        `/api/v1/brands/${brand.id}`,
        {
          data: { archived: true },
          headers: {
            "X-Beweft-Request": "1",
            Origin: process.env.BEWEFT_TEST_URL!,
          },
        },
      );
      expect(archived.ok(), await archived.text()).toBe(true);
      archivedBrand = true;
      await page.clock.fastForward(30_000);
      await expect(page.getByLabel("Current brand")).toHaveValue(fallback.id);
      await expect(page.getByRole("dialog")).toHaveCount(0);
      release.resolve();
      await delivered.promise;
      await expect(page.getByLabel("Current brand")).toHaveValue(fallback.id);
      expect(new URL(page.url()).searchParams.has("connection_flow")).toBe(
        false,
      );
      expect(posts).toBe(1);
    } finally {
      release.resolve();
      await page.unrouteAll({ behavior: "wait" });
    }
  });
});

test("a destination flow is discarded when its browser session ends", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "OAuth session fixture" });
    const flowId = crypto.randomUUID();
    await context.addInitScript(
      (id: string) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.route(`**/connection-flows/${flowId}`, (route) =>
      route.fulfill({
        json: {
          data: {
            id: flowId,
            brand_id: brand.id,
            provider: "youtube",
            destinations: [
              { id: "fixture", name: "Fixture destination", username: null },
            ],
          },
        },
      }),
    );
    const other = await context.newPage();
    try {
      await page.goto(`/?connection_flow=${flowId}#connections`);
      await expect(
        page.getByRole("dialog").getByRole("combobox"),
      ).toBeVisible();
      await other.goto("/#connections");
      await other.getByRole("button", { name: "Sign out" }).click();
      await expect(
        page.getByRole("button", { name: "Sign in", exact: true }),
      ).toBeVisible();
      expect(new URL(page.url()).searchParams.has("connection_flow")).toBe(
        false,
      );
      await page
        .getByLabel("Email address")
        .fill(process.env.BEWEFT_TEST_EMAIL!);
      await page.getByLabel("Password").fill(process.env.BEWEFT_TEST_PASSWORD!);
      await page.getByRole("button", { name: "Sign in", exact: true }).click();
      await expect(page.getByLabel("Current brand")).toHaveValue(brand.id);
      await expect(page.getByRole("dialog")).toHaveCount(0);
    } finally {
      await other.close();
    }
  });
});

test("X composer explains the adapter's conservative video requirements", async () => {
  await withWorkspace(async (page, context, write) => {
    await setupConnection(page, context, write, "x");
    await page.goto("/#drafts");
    await page
      .getByRole("button", { name: "Create post", exact: true })
      .click();
    await page.getByRole("button", { name: "Recovery account" }).click();
    await page.getByRole("tab", { name: "Recovery account" }).click();
    const editor = page.getByRole("dialog");
    await expect(editor).toContainText("1,280 pixels");
    await expect(editor).toContainText("1,024 pixels");
    await expect(editor).toContainText("progressive yuv420p");
    await expect(editor).toContainText(
      "rejects a video that has a description",
    );
  });
});
