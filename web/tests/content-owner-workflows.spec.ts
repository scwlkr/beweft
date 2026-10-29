import { expect, test } from "@playwright/test";
import { deferred, withWorkspace } from "./helpers";

test("templates retain draft variants and create an unscheduled editable draft", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Template fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Template channel",
      timezone: "UTC",
    });
    const source = await write("/posts", {
      brand_id: brand.id,
      title: "Source {{topic}}",
      text: "Hello {{topic}} at {{brand}}",
      variants: [
        {
          channel_id: channel.id,
          text: "Channel {{topic}} and {{unknown}}",
          options: { label: "retained" },
        },
      ],
      options: { custom: "kept" },
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.goto("/#drafts");
    await page.getByRole("button", { name: "Templates", exact: true }).click();
    await page.getByRole("button", { name: "New template" }).click();
    await page.getByLabel("Start from a draft").selectOption(source.id);
    await page.getByLabel("Template name").fill("Reusable announcement");
    await page.getByRole("button", { name: "Save template" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await expect(
      page.getByRole("heading", { name: "Reusable announcement" }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Edit", exact: true }).click();
    await page.getByLabel("Post title").fill("Updated {{topic}}");
    await page.getByRole("button", { name: "Save template" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await page.getByRole("button", { name: "Use template" }).click();
    await page.getByLabel("topic", { exact: true }).fill("Sunshine");
    await page
      .getByRole("button", { name: "Create unscheduled draft" })
      .click();
    await expect(page.getByLabel("Post text", { exact: true })).toHaveValue(
      "Hello Sunshine at Template fixture",
    );
    const posts = (
      await (
        await context.request.get(`/api/v1/posts?brand_id=${brand.id}`)
      ).json()
    ).data;
    const created = posts.find((row: { id: string }) => row.id !== source.id);
    expect(created.title).toBe("Updated Sunshine");
    expect(created.variants[0].text).toBe("Channel Sunshine and {{unknown}}");
    expect(created.variants[0].options).toEqual({ label: "retained" });
    expect(created.options).toEqual({ custom: "kept" });
    expect(
      (
        await (
          await context.request.get(`/api/v1/publications?brand_id=${brand.id}`)
        ).json()
      ).data,
    ).toEqual([]);
    await page.getByRole("button", { name: "Close dialog" }).click();
    await page.setViewportSize({ width: 390, height: 844 });
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await page.getByRole("button", { name: "Delete", exact: true }).click();
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Delete template" })
      .click();
    await expect(
      page.getByRole("heading", { name: "Reusable announcement" }),
    ).toHaveCount(0);
  });
});

test("edit a saved draft, explicitly replace the queued snapshot and prioritize queue slots", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Queue edit fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Queue channel",
      timezone: "UTC",
    });
    await write("/schedules", {
      channel_id: channel.id,
      days: [1, 2, 3, 4, 5, 6, 7],
      time: "09:00",
      timezone: "UTC",
    });
    await write("/schedules", {
      channel_id: channel.id,
      days: [1, 2, 3, 4, 5, 6, 7],
      time: "18:00",
      timezone: "UTC",
    });
    const first = await write("/posts", {
      brand_id: brand.id,
      title: "First",
      text: "First text",
      variants: [{ channel_id: channel.id }],
    });
    const second = await write("/posts", {
      brand_id: brand.id,
      title: "Second",
      text: "Old queued text",
      variants: [{ channel_id: channel.id }],
    });
    const exact = await write("/posts", {
      brand_id: brand.id,
      title: "Exact",
      text: "Exact text",
      variants: [{ channel_id: channel.id }],
    });
    const firstPub = (
      await write(`/posts/${first.id}/schedule`, {
        channel_ids: [channel.id],
        version: first.version,
        request_id: crypto.randomUUID(),
        scheduled_at: null,
        media_versions: {},
      })
    )[0];
    const secondPub = (
      await write(`/posts/${second.id}/schedule`, {
        channel_ids: [channel.id],
        version: second.version,
        request_id: crypto.randomUUID(),
        scheduled_at: null,
        media_versions: {},
      })
    )[0];
    const exactTime = new Date(Date.now() + 12 * 86400_000).toISOString();
    const exactPub = (
      await write(`/posts/${exact.id}/schedule`, {
        channel_ids: [channel.id],
        version: exact.version,
        request_id: crypto.randomUUID(),
        scheduled_at: exactTime,
        media_versions: {},
      })
    )[0];
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.goto("/#queue");
    const secondCard = page
      .locator("article")
      .filter({ hasText: "Old queued text" });
    await secondCard.getByRole("button", { name: "View details" }).click();
    await page.getByRole("button", { name: "Edit saved draft" }).click();
    await page.getByLabel("Post text", { exact: true }).fill("New saved text");
    await page.getByRole("button", { name: "Save draft", exact: true }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await expect(secondCard.getByText("Old queued text")).toBeVisible();
    await secondCard.getByRole("button", { name: "View details" }).click();
    await page.getByRole("button", { name: "Compare and replace" }).click();
    await expect(page.getByText("New saved text")).toBeVisible();
    await page.getByRole("button", { name: "Replace queued content" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    const replacedCard = page
      .locator("article")
      .filter({ hasText: "New saved text" });
    await expect(replacedCard).toBeVisible();
    await replacedCard.getByRole("button", { name: "Move to front" }).click();
    await expect
      .poll(
        async () =>
          (
            await (
              await context.request.get(
                `/api/v1/publications?brand_id=${brand.id}`,
              )
            ).json()
          ).data.find((row: { id: string }) => row.id === firstPub.id)
            .scheduled_at,
      )
      .toBe(secondPub.scheduled_at);
    const rows = (
      await (
        await context.request.get(`/api/v1/publications?brand_id=${brand.id}`)
      ).json()
    ).data;
    expect(
      rows.find((row: { id: string }) => row.id === secondPub.id).state,
    ).toBe("canceled");
    expect(
      rows.find((row: { id: string }) => row.id === exactPub.id).scheduled_at,
    ).toBe(exactPub.scheduled_at);
    expect(
      rows.find(
        (row: { payload: { text: string } }) =>
          row.payload.text === "New saved text",
      ).scheduled_at,
    ).toBe(firstPub.scheduled_at);
    await page.getByRole("button", { name: "History", exact: false }).click();
    await expect(page.getByText("Old queued text")).toBeVisible();
  });
});

test("brands and owner sessions honor archive, restore and password sign-in", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Owner fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Paused on archive",
      timezone: "UTC",
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.goto("/#settings");
    await page.getByRole("button", { name: "Brands", exact: true }).click();
    const row = page.locator(".token-row").filter({ hasText: "Owner fixture" });
    await row.getByRole("button", { name: "Rename" }).click();
    await page.getByLabel("Brand name").fill("Owner fixture renamed");
    await page.getByRole("button", { name: "Save name" }).click();
    const renamed = page
      .locator(".token-row")
      .filter({ hasText: "Owner fixture renamed" });
    await renamed.getByRole("button", { name: "Archive" }).click();
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Archive brand" })
      .click();
    await expect(page.getByLabel("Current brand")).not.toHaveValue(brand.id);
    await page.getByRole("button", { name: "Brands", exact: true }).click();
    const archived = page
      .locator(".token-row")
      .filter({ hasText: "Owner fixture renamed" });
    await archived.getByRole("button", { name: "Restore" }).click();
    await expect(
      page.getByText("Channels stay paused after restoring.", { exact: false }),
    ).toBeVisible();
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Restore brand" })
      .click();
    await expect(archived.getByText("Active", { exact: true })).toBeVisible();
    expect(
      (
        await (
          await context.request.get(`/api/v1/channels?brand_id=${brand.id}`)
        ).json()
      ).data.find((row: { id: string }) => row.id === channel.id).paused,
    ).toBe(true);
    const other = await context
      .browser()!
      .newContext({ baseURL: process.env.BEWEFT_TEST_URL });
    try {
      const login = await other.request.post("/api/v1/auth/login", {
        data: {
          email: process.env.BEWEFT_TEST_EMAIL,
          password: process.env.BEWEFT_TEST_PASSWORD,
        },
        headers: {
          "X-Beweft-Request": "1",
          Origin: process.env.BEWEFT_TEST_URL!,
        },
      });
      expect(login.ok(), await login.text()).toBe(true);
      await page.getByRole("button", { name: "Security", exact: true }).click();
      await expect(
        page.getByText("This session", { exact: true }),
      ).toBeVisible();
      await expect(
        page.getByText("Other session", { exact: true }).first(),
      ).toBeVisible();
      await page.getByRole("button", { name: "Revoke other sessions" }).click();
      await page
        .getByRole("dialog")
        .getByRole("button", { name: "Revoke" })
        .click();
      await expect(page.getByRole("dialog")).toHaveCount(0);
      expect((await other.request.get("/api/v1/auth/me")).status()).toBe(401);
    } finally {
      await other.close();
    }
    await page.getByLabel("Current password").fill("incorrect password");
    await page.getByLabel(/^New password/).fill("replacement password 123");
    await page
      .getByLabel("Confirm new password")
      .fill("replacement password 123");
    await page
      .getByRole("button", { name: "Change password and sign out" })
      .click();
    await expect(
      page.getByText("The current password was not accepted."),
    ).toBeVisible();
    await page
      .getByLabel("Current password")
      .fill(process.env.BEWEFT_TEST_PASSWORD!);
    await page.getByLabel(/^New password/).fill("replacement password 123");
    await page
      .getByLabel("Confirm new password")
      .fill("replacement password 123");
    await page
      .getByRole("button", { name: "Change password and sign out" })
      .click();
    await expect(
      page.getByRole("heading", { name: "Welcome back" }),
    ).toBeVisible();
    await page.getByLabel("Email address").fill(process.env.BEWEFT_TEST_EMAIL!);
    await page
      .getByLabel("Password", { exact: true })
      .fill("replacement password 123");
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(
      page.getByRole("button", { name: "Security", exact: true }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Security", exact: true }).click();
    const currentSession = page
      .locator(".token-row")
      .filter({ hasText: "This session" });
    await currentSession.getByRole("button", { name: "Revoke" }).click();
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Revoke" })
      .click();
    await expect(
      page.getByRole("heading", { name: "Welcome back" }),
    ).toBeVisible();
    await page.getByLabel("Email address").fill(process.env.BEWEFT_TEST_EMAIL!);
    await page
      .getByLabel("Password", { exact: true })
      .fill("replacement password 123");
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(
      page.getByRole("button", { name: "Security", exact: true }),
    ).toBeVisible();
    // Restore the disposable test owner's password so subsequent tests can authenticate.
    const reset = await context.request.post("/api/v1/auth/password", {
      data: {
        current_password: "replacement password 123",
        new_password: process.env.BEWEFT_TEST_PASSWORD!,
      },
      headers: {
        "X-Beweft-Request": "1",
        Origin: process.env.BEWEFT_TEST_URL!,
      },
    });
    expect(reset.ok(), await reset.text()).toBe(true);
  });
});

test("a committed replacement with a lost response blocks new scheduling until the original is checked", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Lost replacement fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Lost response channel",
      timezone: "UTC",
    });
    const post = await write("/posts", {
      brand_id: brand.id,
      title: "Lost response",
      text: "Original snapshot",
      variants: [{ channel_id: channel.id }],
    });
    const original = (
      await write(`/posts/${post.id}/schedule`, {
        channel_ids: [channel.id],
        version: post.version,
        request_id: crypto.randomUUID(),
        scheduled_at: new Date(Date.now() + 86400000).toISOString(),
        media_versions: {},
      })
    )[0];
    const update = await context.request.patch(`/api/v1/posts/${post.id}`, {
      data: { version: post.version, text: "Changed draft" },
      headers: {
        "X-Beweft-Request": "1",
        Origin: process.env.BEWEFT_TEST_URL!,
      },
    });
    expect(update.ok(), await update.text()).toBe(true);
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.route(
      `**/api/v1/publications/${original.id}/replace`,
      async (route) => {
        const response = await route.fetch();
        expect(response.ok(), await response.text()).toBe(true);
        await route.abort("failed");
      },
    );
    await page.goto("/#queue");
    await page.getByRole("button", { name: "View details" }).click();
    await page.getByRole("button", { name: "Compare and replace" }).click();
    await page.getByRole("button", { name: "Replace queued content" }).click();
    await expect(
      page.getByText(/queued replacement has an unconfirmed outcome/i),
    ).toBeVisible();
    await page.reload();
    await expect(
      page.getByText(/queued replacement has an unconfirmed outcome/i),
    ).toBeVisible();
    await page
      .getByRole("button", { name: "Create post", exact: true })
      .click();
    await page
      .getByLabel("Post text", { exact: true })
      .fill("Should stay unscheduled");
    await page.getByRole("button", { name: "Lost response channel" }).click();
    await page.getByLabel("When should this go out?").selectOption("exact");
    const date = new Date(Date.now() + 3 * 86400000);
    await page
      .getByLabel(/Date and time/)
      .fill(
        new Date(date.getTime() - date.getTimezoneOffset() * 60000)
          .toISOString()
          .slice(0, 16),
      );
    await page.getByRole("button", { name: "Schedule post" }).click();
    await expect(
      page.getByText(
        /queued replacement has an unconfirmed outcome.*review it before scheduling/i,
      ),
    ).toBeVisible();
    await page.getByRole("button", { name: "Close dialog" }).click();
    await page.getByRole("button", { name: "Check replacement" }).click();
    await expect(
      page.getByText(/queued replacement has an unconfirmed outcome/i),
    ).toHaveCount(0);
    const rows = (
      await (
        await context.request.get(`/api/v1/publications?brand_id=${brand.id}`)
      ).json()
    ).data;
    expect(
      rows.filter((row: { post_id: string }) => row.post_id === post.id),
    ).toHaveLength(2);
    expect(
      rows.find((row: { id: string }) => row.id === original.id).state,
    ).toBe("canceled");
    expect(
      rows.filter((row: { state: string }) => row.state === "queued"),
    ).toHaveLength(1);
  });
});

test("a changed draft rejects stale queued replacement and reloads it for review", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Stale replacement fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Stale review channel",
      timezone: "UTC",
    });
    const post = await write("/posts", {
      brand_id: brand.id,
      title: "Needs review",
      text: "Queued original",
      variants: [{ channel_id: channel.id }],
    });
    const original = (
      await write(`/posts/${post.id}/schedule`, {
        channel_ids: [channel.id],
        version: post.version,
        request_id: crypto.randomUUID(),
        scheduled_at: new Date(Date.now() + 86400000).toISOString(),
        media_versions: {},
      })
    )[0];
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.goto("/#queue");
    await page.getByRole("button", { name: "View details" }).click();
    await page.getByRole("button", { name: "Compare and replace" }).click();
    const update = await context.request.patch(`/api/v1/posts/${post.id}`, {
      data: { version: post.version, text: "Another editor changed this" },
      headers: {
        "X-Beweft-Request": "1",
        Origin: process.env.BEWEFT_TEST_URL!,
      },
    });
    expect(update.ok(), await update.text()).toBe(true);
    await page.getByRole("button", { name: "Replace queued content" }).click();
    await expect(page.getByText(/Replacement was not confirmed/)).toBeVisible();
    await page.getByRole("button", { name: "Compare and replace" }).click();
    await expect(page.getByText("Another editor changed this")).toBeVisible();
    const rows = (
      await (
        await context.request.get(`/api/v1/publications?brand_id=${brand.id}`)
      ).json()
    ).data;
    expect(rows).toHaveLength(1);
    expect(rows[0].id).toBe(original.id);
    expect(rows[0].state).toBe("queued");
  });
});

test("a late template application cannot open its draft in a different brand", async () => {
  await withWorkspace(async (page, context, write) => {
    const first = await write("/brands", { name: "Old template brand" });
    const next = await write("/brands", { name: "New template brand" });
    const template = await write("/templates", {
      brand_id: first.id,
      name: "Old template",
      title: "Old draft",
      text: "Only in the old brand",
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      first.id,
    );
    const received = deferred();
    const release = deferred();
    await page.route(`**/templates/${template.id}/apply`, async (route) => {
      const response = await route.fetch();
      received.resolve();
      await release.promise;
      await route.fulfill({ response });
    });
    await page.goto("/#drafts");
    await page.getByRole("button", { name: "Templates", exact: true }).click();
    await page.getByRole("button", { name: "Use template" }).click();
    await page
      .getByRole("button", { name: "Create unscheduled draft" })
      .click();
    await received.promise;
    await page.getByLabel("Current brand").selectOption(next.id);
    release.resolve();
    await expect(page.getByLabel("Current brand")).toHaveValue(next.id);
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await expect(page.getByText("Only in the old brand")).toHaveCount(0);
    const nextPosts = (
      await (
        await context.request.get(`/api/v1/posts?brand_id=${next.id}`)
      ).json()
    ).data;
    expect(nextPosts).toEqual([]);
  });
});
