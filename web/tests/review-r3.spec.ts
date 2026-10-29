import {
  test,
  expect,
  type BrowserContext,
  type Page,
  type Route,
} from "@playwright/test";
import { deferred, withWorkspace } from "./helpers";

function replyFixture(itemId: string, channelId: string) {
  return {
    id: itemId,
    channel_id: channelId,
    channel_name: "Race fixture",
    provider: "mastodon",
    item: { id: "remote", author: "Fixture", text: "Conversation" },
    read_at: null,
    synced_at: new Date().toISOString(),
    can_reply: true,
  };
}
async function openReply(page: Page, text: string) {
  await page.goto("/#inbox");
  await page.getByRole("button", { name: "Reply", exact: true }).click();
  await expect(page.getByLabel("Your reply")).toBeEnabled();
  await page.getByLabel("Your reply").fill(text);
}
function guardedServer(itemId: string) {
  const calls: any[] = [],
    rows: any[] = [];
  const held = deferred(),
    claimed = deferred();
  let publications = 0;
  const post = async (route: Route) => {
    const body = route.request().postDataJSON();
    const index = calls.push(body);
    if (index === 1) {
      claimed.resolve();
      await held.promise;
    }
    const existing = rows.find((row) => row.id === body.request_id);
    if (existing) return route.fulfill({ json: { data: existing } });
    if (body.expected_last_request_id !== (rows[0]?.id ?? null))
      return route.fulfill({
        status: 409,
        json: {
          error: {
            code: "reply_history_changed",
            message: "Reply history changed",
          },
        },
      });
    const row = {
      id: body.request_id,
      inbox_id: itemId,
      text: body.text,
      state: "published",
      error: null,
      receipt: { fixture: true },
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
    };
    rows.unshift(row);
    publications++;
    return route.fulfill({ json: { data: row } });
  };
  return { calls, rows, held, claimed, post, publications: () => publications };
}
async function install(
  context: BrowserContext,
  item: ReturnType<typeof replyFixture>,
  server: ReturnType<typeof guardedServer>,
) {
  await context.route("**/inbox?*", (route) =>
    route.fulfill({
      json: { data: { availability: "partial", items: [item] } },
    }),
  );
  await context.route(`**/inbox/${item.id}/replies`, (route) =>
    route.fulfill({ json: { data: server.rows } }),
  );
  await context.route(`**/inbox/${item.id}/reply`, server.post);
}

test("two tabs preserve a discovered unrecorded intent instead of silently sending another (controlled provider fixtures)", async () => {
  await withWorkspace(async (a, context, write) => {
    const brand = await write("/brands", { name: "Shared intent race" });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    const item = replyFixture(crypto.randomUUID(), crypto.randomUUID());
    const server = guardedServer(item.id);
    await install(context, item, server);
    const b = await context.newPage();
    b.setDefaultTimeout(10000);
    try {
      await openReply(a, "Original intent A");
      await openReply(b, "Unacknowledged draft B");
      await a.getByRole("button", { name: "Send reply", exact: true }).click();
      await server.claimed.promise;
      await b.getByRole("button", { name: "Send reply", exact: true }).click();
      await expect(
        b.getByText(/Another tab has an existing reply intent/),
      ).toBeVisible();
      await expect(b.getByLabel("Your reply")).toHaveValue("Original intent A");
      await expect(b.getByLabel("Your reply")).toBeDisabled();
      expect(server.calls).toHaveLength(1);
      const stored = await b.evaluate(
        () =>
          Object.entries(localStorage).find(([key]) =>
            key.startsWith("beweft.pending-reply:"),
          )?.[1],
      );
      expect(JSON.parse(stored!).request_id).toBe(server.calls[0].request_id);
      await b
        .getByRole("button", { name: "Retry original reply", exact: true })
        .click();
      await expect(
        b.getByText("Server reply outcome: published", { exact: true }),
      ).toBeVisible();
      expect(server.calls).toHaveLength(2);
      expect(server.calls[1]).toEqual(server.calls[0]);
      server.held.resolve();
      await expect(
        a.getByText("Server reply outcome: published", { exact: true }),
      ).toBeVisible();
      expect(server.publications()).toBe(1);
    } finally {
      server.held.resolve();
      await b.close();
    }
  });
});

test("independent tabs reject stale history and require review before explicit original-intent retry (controlled provider fixtures)", async () => {
  await withWorkspace(async (a, context, write) => {
    const brand = await write("/brands", { name: "Server watermark race" });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    const item = replyFixture(crypto.randomUUID(), crypto.randomUUID());
    const server = guardedServer(item.id);
    await install(context, item, server);
    const isolated = await context
      .browser()!
      .newContext({
        baseURL: process.env.BEWEFT_TEST_URL,
        storageState: await context.storageState(),
      });
    await isolated.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await install(isolated, item, server);
    const b = await isolated.newPage();
    b.setDefaultTimeout(10000);
    try {
      await openReply(a, "Same intended response");
      await openReply(b, "Same intended response");
      await a.getByRole("button", { name: "Send reply", exact: true }).click();
      await server.claimed.promise;
      await b.getByRole("button", { name: "Send reply", exact: true }).click();
      await expect(
        b.getByText("Server reply outcome: published", { exact: true }),
      ).toBeVisible();
      server.held.resolve();
      await expect(
        a.getByText(/Another reply changed the server history/),
      ).toBeVisible();
      expect(server.publications()).toBe(1);
      expect(server.calls.map((call) => call.expected_last_request_id)).toEqual(
        [null, null],
      );
      const originalId = server.calls[0].request_id;
      await a
        .getByRole("button", { name: "Check existing reply outcome" })
        .click();
      await expect(a.getByText(/Latest server reply: published/)).toBeVisible();
      await expect(
        a.getByRole("button", { name: "Retry original reply", exact: true }),
      ).toBeDisabled();
      await a
        .getByRole("button", {
          name: "I reviewed the newer outcome; keep my original intent",
        })
        .click();
      expect(server.calls).toHaveLength(2);
      await a
        .getByRole("button", { name: "Retry original reply", exact: true })
        .click();
      await expect(
        a.getByText("Server reply outcome: published", { exact: true }),
      ).toBeVisible();
      expect(server.calls[2].request_id).toBe(originalId);
      expect(server.calls[2].expected_last_request_id).toBe(
        server.calls[1].request_id,
      );
      expect(server.publications()).toBe(2); // Second send requires both review and explicit retry.
    } finally {
      server.held.resolve();
      await isolated.close();
    }
  });
});

test("composer locks every editor control until the real save response completes", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Composer save lock" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      name: "Video options fixture",
      provider: "manual",
      timezone: "UTC",
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.route("**/channels?*", (route) =>
      route.fulfill({ json: { data: [{ ...channel, provider: "youtube" }] } }),
    );
    await page.goto("/#queue");
    await page
      .getByRole("button", { name: "Create post", exact: true })
      .click();
    await page
      .getByLabel("Post text", { exact: true })
      .fill("Submitted snapshot");
    await page.getByRole("button", { name: /Video options fixture$/ }).click();
    await page.getByRole("tab", { name: "Video options fixture" }).click();
    await page.getByRole("button", { name: "Add media", exact: true }).click();
    const held = deferred(),
      saved = deferred();
    await page.route("**/api/v1/posts", async (route) => {
      if (route.request().method() !== "POST") return route.continue();
      const response = await route.fetch();
      saved.resolve();
      await held.promise;
      await route.fulfill({ response });
    });
    try {
      await page
        .getByRole("button", { name: "Save draft", exact: true })
        .click();
      await saved.promise;
      const editor = page.getByRole("group", { name: "Post editor" });
      const controls = editor.locator("input, textarea, select, button");
      expect(await controls.count()).toBeGreaterThan(8);
      for (const control of await controls.all())
        await expect(control).toBeDisabled();
      await expect(page.getByLabel("Video options fixture text")).toHaveValue(
        "Submitted snapshot",
      );
      await page.keyboard.press("Escape");
      await expect(page.getByRole("dialog")).toBeVisible();
      held.resolve();
      await expect(page.getByRole("dialog")).toHaveCount(0);
      const posts = await context.request.get(
        `/api/v1/posts?brand_id=${brand.id}`,
      );
      expect((await posts.json()).data[0].text).toBe("Submitted snapshot");
    } finally {
      held.resolve();
    }
  });
});
