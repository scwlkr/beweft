import { test, expect } from "@playwright/test";
import { withWorkspace } from "./helpers";

test("lost reply response recovers one durable attempt across close and reload (controlled API fixtures)", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Reply recovery regression" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Reply fixture",
      timezone: "UTC",
    });
    const itemId = crypto.randomUUID();
    const message = {
      id: itemId,
      channel_id: channel.id,
      channel_name: channel.name,
      provider: "mastodon",
      item: {
        id: "remote-message",
        author: "Test author",
        text: "Fixture conversation",
      },
      read_at: null,
      synced_at: new Date().toISOString(),
      can_reply: true,
    };
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.route("**/inbox?*", (route) =>
      route.fulfill({
        json: { data: { availability: "partial", items: [message] } },
      }),
    );
    let rows: any[] = [],
      sends: any[] = [],
      checks = 0,
      failRead = false;
    await page.route(`**/inbox/${itemId}/replies`, (route) => {
      checks++;
      return failRead
        ? route.fulfill({
            status: 503,
            json: {
              error: {
                code: "unavailable",
                message: "Recovery temporarily unavailable",
              },
            },
          })
        : route.fulfill({ json: { data: rows } });
    });
    await page.route(`**/inbox/${itemId}/reply`, (route) => {
      const request = route.request().postDataJSON();
      sends.push(request);
      rows = [
        {
          id: request.request_id,
          inbox_id: itemId,
          revision: 1,
          text: request.text,
          state: "uncertain",
          error: "Provider response was lost",
          receipt: null,
          created_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
        },
      ];
      return route.abort("failed");
    });
    let resolutions = 0;
    await page.route(`**/inbox/${itemId}/replies/*/resolve`, (route) => {
      const body = route.request().postDataJSON();
      resolutions++;
      expect(body.resolution).toBe("not_published");
      expect(body.observed_revision).toBe(1);
      expect(body.sender_stopped).toBe(true);
      rows[0] = {
        ...rows[0],
        state: "failed",
        error: null,
        receipt: { verification: "owner_attestation", ...body },
      };
      return route.fulfill({ json: { data: rows[0] } });
    });
    await page.goto("/#inbox");
    await page.getByRole("button", { name: "Reply", exact: true }).click();
    await expect(page.getByLabel("Your reply")).toBeEnabled();
    await page.getByLabel("Your reply").fill("One intended response");
    await page.getByRole("button", { name: "Send reply", exact: true }).click();
    await expect(page.getByText(/reply outcome is unconfirmed/)).toBeVisible();
    expect(sends).toHaveLength(1);
    const requestId = sends[0].request_id;
    const stored = await page.evaluate(() =>
      Object.entries(localStorage).filter(([key]) =>
        key.startsWith("beweft.pending-reply:"),
      ),
    );
    expect(stored).toHaveLength(1);
    expect(stored[0][0]).toContain(`:${brand.id}:${channel.id}:${itemId}`);
    expect(JSON.parse(stored[0][1]).request_id).toBe(requestId);
    await page.getByRole("button", { name: "Close dialog" }).click();
    await page.getByRole("button", { name: "Reply", exact: true }).click();
    await expect(
      page.getByText("Server reply outcome: uncertain", { exact: true }),
    ).toBeVisible();
    await expect(page.getByLabel("Your reply")).toHaveValue(
      "One intended response",
    );
    await expect(page.getByLabel("Your reply")).toBeDisabled();
    await expect(
      page.getByRole("button", { name: "Send reply", exact: true }),
    ).toHaveCount(0);
    await page.reload();
    await page.getByRole("button", { name: "Reply", exact: true }).click();
    await expect(
      page.getByText(`Request ${requestId}`, { exact: true }),
    ).toBeVisible();
    await page
      .getByRole("button", { name: "Check existing reply outcome" })
      .click();
    await expect(
      page.getByText("Server reply outcome: uncertain", { exact: true }),
    ).toBeVisible();
    expect(sends).toHaveLength(1);
    // Server discovery also works without the browser's pending identity record.
    await page.evaluate(() =>
      Object.keys(localStorage)
        .filter((key) => key.startsWith("beweft.pending-reply:"))
        .forEach((key) => localStorage.removeItem(key)),
    );
    await page.reload();
    await page.getByRole("button", { name: "Reply", exact: true }).click();
    await expect(
      page.getByText(`Request ${requestId}`, { exact: true }),
    ).toBeVisible();
    failRead = true;
    await page
      .getByRole("button", { name: "Check existing reply outcome" })
      .click();
    await expect(
      page.getByText(/Recovery temporarily unavailable/),
    ).toBeVisible();
    await expect(page.getByLabel("Your reply")).toBeDisabled();
    failRead = false;
    await page
      .getByRole("button", { name: "Check existing reply outcome" })
      .click();
    await expect(
      page.getByText("Server reply outcome: uncertain", { exact: true }),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Record reply attestation" }),
    ).toBeDisabled();
    await page
      .getByRole("combobox", { name: "Verified reply outcome" })
      .selectOption("not_published");
    await page
      .getByLabel("Reply verification note")
      .fill("Checked the original conversation; this reply does not exist.");
    await page
      .getByRole("checkbox", {
        name: "I checked the network and verified this outcome.",
      })
      .check();
    await page
      .getByRole("checkbox", {
        name: /I confirmed the original sender has stopped/,
      })
      .check();
    await page
      .getByRole("button", { name: "Record reply attestation" })
      .click();
    await expect(
      page.getByText("Server reply outcome: failed", { exact: true }),
    ).toBeVisible();
    await expect(
      page.getByText(/Owner attestation: this outcome was recorded/),
    ).toBeVisible();
    expect(resolutions).toBe(1);
    expect(sends).toHaveLength(1);
    await page.getByRole("button", { name: "Write another reply" }).click();
    await expect(page.getByLabel("Your reply")).toBeEnabled();
    await expect(page.getByLabel("Your reply")).toHaveValue("");
    expect(sends).toHaveLength(1);
    expect(checks).toBeGreaterThan(4);
  });
});

test("unrecorded request retries its original UUID only after read-only recovery (controlled API fixtures)", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", {
      name: "Unrecorded reply regression",
    });
    const itemId = crypto.randomUUID(),
      channelId = crypto.randomUUID();
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.route("**/inbox?*", (route) =>
      route.fulfill({
        json: {
          data: {
            availability: "partial",
            items: [
              {
                id: itemId,
                channel_id: channelId,
                channel_name: "Isolated fixture",
                provider: "mastodon",
                item: { id: "remote", author: "Fixture", text: "Original" },
                read_at: null,
                synced_at: new Date().toISOString(),
                can_reply: true,
              },
            ],
          },
        },
      }),
    );
    const sends: any[] = [];
    let checks = 0;
    await page.route(`**/inbox/${itemId}/replies`, (route) => {
      checks++;
      return route.fulfill({ json: { data: [] } });
    });
    await page.route(`**/inbox/${itemId}/reply`, (route) => {
      sends.push(route.request().postDataJSON());
      return route.abort("failed");
    });
    await page.goto("/#inbox");
    await page.getByRole("button", { name: "Reply", exact: true }).click();
    await expect(page.getByLabel("Your reply")).toBeEnabled();
    await page.getByLabel("Your reply").fill("Keep original identity");
    await page.getByRole("button", { name: "Send reply", exact: true }).click();
    await expect(page.getByText(/reply outcome is unconfirmed/)).toBeVisible();
    await page.reload();
    await page.getByRole("button", { name: "Reply", exact: true }).click();
    await expect(
      page.getByRole("button", { name: "Retry original reply" }),
    ).toBeEnabled();
    expect(sends).toHaveLength(1);
    await expect(page.getByLabel("Your reply")).toBeDisabled();
    await page.getByRole("button", { name: "Retry original reply" }).click();
    await expect(page.getByText(/reply outcome is unconfirmed/)).toBeVisible();
    expect(sends).toHaveLength(2);
    expect(sends[1]).toEqual(sends[0]);
    expect(checks).toBeGreaterThanOrEqual(4);
  });
});

test("deselecting active channel returns to shared text and mixed permission grants stay visible", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Composer tab regression" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Deselect me",
      timezone: "UTC",
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.goto("/#queue");
    await page
      .getByRole("button", { name: "Create post", exact: true })
      .click();
    await page.getByLabel("Post text", { exact: true }).fill("Shared original");
    await page.getByRole("button", { name: /Deselect me$/ }).click();
    await page.getByRole("tab", { name: "Deselect me" }).click();
    await page.getByLabel("Deselect me text").fill("Specific channel original");
    await page.getByRole("button", { name: /Deselect me$/ }).click();
    await expect(
      page.getByRole("tab", { name: "Shared version" }),
    ).toHaveAttribute("aria-selected", "true");
    await expect(page.getByLabel("Post text", { exact: true })).toHaveValue(
      "Shared original",
    );
    await page
      .getByLabel("Post text", { exact: true })
      .fill("Shared edit must survive");
    await page.getByRole("button", { name: "Save draft", exact: true }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    const response = await context.request.get(
      `/api/v1/posts?brand_id=${brand.id}`,
    );
    expect((await response.json()).data[0].text).toBe(
      "Shared edit must survive",
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
                  source: "fixture",
                  reported_scopes: ["read"],
                  actions: {
                    publish: {
                      status: "unknown",
                      required_scopes: ["write"],
                      missing_scopes: null,
                    },
                    metrics: {
                      status: "granted",
                      required_scopes: ["read"],
                      missing_scopes: [],
                    },
                    reply: {
                      status: "missing",
                      required_scopes: ["reply"],
                      missing_scopes: ["reply"],
                    },
                  },
                },
              },
            },
          ],
        },
      }),
    );
    await page.reload();
    await page
      .getByRole("button", { name: "Connections", exact: true })
      .click();
    await page.getByText("Account permissions", { exact: true }).click();
    await expect(
      page.getByText("Unknown · Provider has not confirmed this permission", {
        exact: true,
      }),
    ).toBeVisible();
    await expect(page.getByText("granted", { exact: true })).toBeVisible();
    await expect(
      page.getByText("missing · Reconnect for: reply", { exact: true }),
    ).toBeVisible();
  });
});
