import { expect, test } from "@playwright/test";
import { withWorkspace } from "./helpers";

test("Meta recovery requires current checkpoint evidence and never retries automatically", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Meta UI fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Meta fixture",
      timezone: "UTC",
    });
    const post = await write("/posts", {
      brand_id: brand.id,
      title: "Meta post",
      text: "Local fixture",
      variants: [],
    });
    const [original] = await write(`/posts/${post.id}/schedule`, {
      version: post.version,
      channel_ids: [channel.id],
      scheduled_at: new Date(Date.now() + 86400000).toISOString(),
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    let publication = { ...original, provider: "facebook", state: "uncertain" };
    const first = {
      phase: "post_started",
      attempt_id: crypto.randomUUID(),
      checkpoint_id: crypto.randomUUID(),
      retry_allowed: false,
      reason: "Inspect this attempt.",
    };
    const second = {
      ...first,
      attempt_id: crypto.randomUUID(),
      checkpoint_id: crypto.randomUUID(),
    };
    let current: typeof first | null = first;
    const reconciles: unknown[] = [];
    let retries = 0;
    await page.route("**/publications?*", (route) =>
      route.fulfill({ json: { data: [publication] } }),
    );
    await page.route(`**/publications/${original.id}/attempts`, (route) =>
      route.fulfill({ json: { data: [] } }),
    );
    await page.route(
      `**/publications/${original.id}/provider-status`,
      (route) => route.fulfill({ json: { data: { meta_delivery: current } } }),
    );
    await page.route(`**/publications/${original.id}/reconcile`, (route) => {
      reconciles.push(route.request().postDataJSON());
      if (reconciles.length === 1) {
        current = second;
        return route.fulfill({
          status: 409,
          json: { error: { code: "conflict", message: "Attempt changed" } },
        });
      }
      current = { ...second, retry_allowed: true };
      publication = {
        ...publication,
        state: "failed",
        updated_at: new Date().toISOString(),
      };
      return route.fulfill({ json: { data: publication } });
    });
    await page.route(`**/publications/${original.id}/retry`, (route) => {
      retries++;
      publication = { ...publication, state: "queued" };
      return route.fulfill({ json: { data: publication } });
    });
    await page.goto("/#queue");
    await page.getByRole("button", { name: "Reconcile outcome" }).click();
    await expect(page.getByText(first.checkpoint_id)).toBeVisible();
    await expect(
      page.getByText(/may still complete after the sender stops/),
    ).toBeVisible();
    await page.getByLabel("Verified outcome").selectOption("not_published");
    await page.getByLabel("Verification note").fill("Checked the account");
    const stop = page.getByRole("checkbox", {
      name: /original sender has stopped/,
    });
    const inspected = page.getByRole("checkbox", {
      name: /inspected the network/,
    });
    await expect(
      page.getByRole("button", { name: "Record outcome" }),
    ).toBeDisabled();
    await stop.check();
    await inspected.check();
    await page.getByRole("button", { name: "Record outcome" }).click();
    await expect(page.getByText(second.checkpoint_id)).toBeVisible();
    await page.getByLabel("Verified outcome").selectOption("not_published");
    await page
      .getByLabel("Verification note")
      .fill("Checked the account again");
    await stop.check();
    await inspected.check();
    await page.getByRole("button", { name: "Record outcome" }).click();
    await expect(
      page.getByRole("button", { name: "Retry publication" }),
    ).toBeEnabled();
    expect(retries).toBe(0);
    expect(reconciles).toEqual(
      [first, second].map((cp, index) => ({
        resolution: "not_published",
        note: index ? "Checked the account again" : "Checked the account",
        observed_attempt_id: cp.attempt_id,
        observed_checkpoint_id: cp.checkpoint_id,
        sender_stopped: true,
        provider_inspected: true,
      })),
    );
    await page.getByRole("button", { name: "Retry publication" }).click();
    expect(retries).toBe(1);
  });
});

test("Facebook Reel selection appears only when the channel advertises it", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Reel catalog fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Facebook fixture",
      timezone: "UTC",
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    let formats = ["text", "image"];
    await page.route("**/channels?*", (route) =>
      route.fulfill({
        json: {
          data: [
            {
              ...channel,
              provider: "facebook",
              capabilities: {
                formats,
                permissions: { actions: { reel: { status: "unknown" } } },
              },
            },
          ],
        },
      }),
    );
    await page.goto("/#drafts");
    await page
      .getByRole("button", { name: "Create post", exact: true })
      .first()
      .click();
    await page.getByRole("button", { name: "Facebook fixture" }).click();
    await page.getByRole("tab", { name: "Facebook fixture" }).click();
    await expect(page.getByLabel("Facebook post format")).toHaveCount(0);
    await page.getByRole("button", { name: "Close dialog" }).click();
    formats = ["text", "image", "reel"];
    await page.getByRole("button", { name: "Refresh workspace" }).click();
    await page
      .getByRole("button", { name: "Create post", exact: true })
      .first()
      .click();
    await page.getByRole("button", { name: "Facebook fixture" }).click();
    await page.getByRole("tab", { name: "Facebook fixture" }).click();
    await page.getByLabel("Facebook post format").selectOption("reel");
    await expect(
      page.getByText(/permission status is not confirmed/i),
    ).toBeVisible();
    await page
      .getByLabel("Facebook fixture text", { exact: true })
      .fill("Unscheduled fixture");
    await page.getByRole("button", { name: "Save draft" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    const posts = (
      await (
        await context.request.get(`/api/v1/posts?brand_id=${brand.id}`)
      ).json()
    ).data;
    expect(posts[0].variants[0].options.format).toBe("reel");
  });
});

test("an older server cannot enable Meta retry without checkpoint status", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Old Meta server fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Old server",
      timezone: "UTC",
    });
    const post = await write("/posts", {
      brand_id: brand.id,
      title: "Old status",
      text: "Fixture",
      variants: [],
    });
    const [pub] = await write(`/posts/${post.id}/schedule`, {
      version: post.version,
      channel_ids: [channel.id],
      scheduled_at: new Date(Date.now() + 86400000).toISOString(),
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.route("**/publications?*", (route) =>
      route.fulfill({
        json: { data: [{ ...pub, provider: "instagram", state: "failed" }] },
      }),
    );
    await page.route(`**/publications/${pub.id}/attempts`, (route) =>
      route.fulfill({ json: { data: [] } }),
    );
    await page.route(`**/publications/${pub.id}/provider-status`, (route) =>
      route.fulfill({
        json: {
          data: { session_expired: false, reset_allowed: false, reason: null },
        },
      }),
    );
    await page.goto("/#queue");
    await page.getByRole("button", { name: "View details" }).click();
    await expect(
      page.getByText(/cannot report Meta checkpoints/),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Retry publication" }),
    ).toBeDisabled();
  });
});

test("a provider-confirmed Meta publication appears without an owner retry", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", {
      name: "Meta provider check fixture",
    });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Meta status",
      timezone: "UTC",
    });
    const post = await write("/posts", {
      brand_id: brand.id,
      title: "Provider check",
      text: "Fixture",
      variants: [],
    });
    const [pub] = await write(`/posts/${post.id}/schedule`, {
      version: post.version,
      channel_ids: [channel.id],
      scheduled_at: new Date(Date.now() + 86400000).toISOString(),
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    let current = { ...pub, provider: "facebook", state: "uncertain" };
    let retries = 0;
    await page.route("**/publications?*", (route) =>
      route.fulfill({ json: { data: [current] } }),
    );
    await page.route(`**/publications/${pub.id}/attempts`, (route) =>
      route.fulfill({ json: { data: [] } }),
    );
    await page.route(`**/publications/${pub.id}/provider-status`, (route) =>
      route.fulfill({
        json: {
          data: {
            meta_delivery: {
              phase: "post_started",
              attempt_id: crypto.randomUUID(),
              checkpoint_id: crypto.randomUUID(),
              retry_allowed: false,
              reason: "Review",
            },
          },
        },
      }),
    );
    await page.route(
      `**/publications/${pub.id}/provider-reconcile`,
      (route) => {
        current = {
          ...current,
          state: "published",
          external_id: "fixture-receipt",
          updated_at: new Date().toISOString(),
        };
        return route.fulfill({ json: { data: current } });
      },
    );
    await page.route(`**/publications/${pub.id}/retry`, (route) => {
      retries++;
      return route.abort();
    });
    await page.goto("/#queue");
    await page.getByRole("button", { name: "Reconcile outcome" }).click();
    await page.getByRole("button", { name: "Check provider outcome" }).click();
    await expect(page.getByText("fixture-receipt")).toBeVisible();
    expect(retries).toBe(0);
  });
});

test("a saved Reel stays intact when an older channel omits the Reel action", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Legacy Reel fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Legacy Facebook",
      timezone: "UTC",
    });
    const draft = await write("/posts", {
      brand_id: brand.id,
      title: "Saved Reel",
      text: "Draft only",
      variants: [{ channel_id: channel.id, options: { format: "reel" } }],
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
              provider: "facebook",
              capabilities: {
                formats: ["text", "image", "reel"],
                permissions: { actions: {} },
              },
            },
          ],
        },
      }),
    );
    await page.goto("/#drafts");
    await page.getByRole("button", { name: "Edit draft" }).click();
    await page.getByRole("tab", { name: "Legacy Facebook" }).click();
    await expect(page.getByLabel("Facebook post format")).toHaveCount(0);
    await expect(page.getByText(/draft is saved as a Reel/i)).toBeVisible();
    await page.getByRole("button", { name: "Save draft" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    let saved = (
      await (await context.request.get(`/api/v1/posts/${draft.id}`)).json()
    ).data;
    expect(saved.variants[0].options.format).toBe("reel");
    await page.getByRole("button", { name: "Edit draft" }).click();
    await page.getByRole("tab", { name: "Legacy Facebook" }).click();
    await page.getByRole("button", { name: "Change to text or image" }).click();
    await page.getByRole("button", { name: "Save draft" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    saved = (
      await (await context.request.get(`/api/v1/posts/${draft.id}`)).json()
    ).data;
    expect(saved.variants[0].options).not.toHaveProperty("format");
  });
});

test("LinkedIn checkpoint recovery requires its own advertised status and exact evidence", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "LinkedIn recovery fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "LinkedIn fixture",
      timezone: "UTC",
    });
    const post = await write("/posts", {
      brand_id: brand.id,
      title: "LinkedIn post",
      text: "Fixture",
      variants: [],
    });
    const [original] = await write(`/posts/${post.id}/schedule`, {
      version: post.version,
      channel_ids: [channel.id],
      scheduled_at: new Date(Date.now() + 86400000).toISOString(),
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    let publication = { ...original, provider: "linkedin", state: "uncertain" };
    const checkpoint = {
      phase: "post_started",
      attempt_id: crypto.randomUUID(),
      checkpoint_id: crypto.randomUUID(),
      retry_allowed: false,
      reason: "Inspect LinkedIn",
    };
    let advertised = false;
    let attempts = 0;
    let retries = 0;
    let body: unknown;
    await page.route("**/publications?*", (route) =>
      route.fulfill({ json: { data: [publication] } }),
    );
    await page.route(`**/publications/${original.id}/attempts`, (route) =>
      route.fulfill({ json: { data: [] } }),
    );
    await page.route(
      `**/publications/${original.id}/provider-status`,
      (route) =>
        route.fulfill({
          json: {
            data: advertised
              ? {
                  meta_delivery: null,
                  linkedin_delivery: {
                    ...checkpoint,
                    retry_allowed: attempts > 0,
                  },
                }
              : { meta_delivery: null },
          },
        }),
    );
    await page.route(`**/publications/${original.id}/reconcile`, (route) => {
      attempts++;
      body = route.request().postDataJSON();
      publication = {
        ...publication,
        state: "failed",
        updated_at: new Date().toISOString(),
      };
      return route.fulfill({ json: { data: publication } });
    });
    await page.route(`**/publications/${original.id}/retry`, (route) => {
      retries++;
      publication = { ...publication, state: "queued" };
      return route.fulfill({ json: { data: publication } });
    });
    await page.goto("/#queue");
    await page.getByRole("button", { name: "Reconcile outcome" }).click();
    await expect(
      page.getByText(/cannot report LinkedIn checkpoints/),
    ).toBeVisible();
    await expect(
      page.getByRole("option", { name: /not published/ }),
    ).toHaveCount(0);
    advertised = true;
    await page.getByRole("button", { name: "Check delivery status" }).click();
    await expect(page.getByText(checkpoint.checkpoint_id)).toBeVisible();
    await page.getByLabel("Verified outcome").selectOption("not_published");
    await page.getByLabel("Verification note").fill("Checked LinkedIn");
    await page
      .getByRole("checkbox", { name: /original sender has stopped/ })
      .check();
    await page.getByRole("checkbox", { name: /inspected the network/ }).check();
    await page.getByRole("button", { name: "Record outcome" }).click();
    await expect(
      page.getByRole("button", { name: "Retry publication" }),
    ).toBeEnabled();
    expect(body).toEqual({
      resolution: "not_published",
      note: "Checked LinkedIn",
      observed_attempt_id: checkpoint.attempt_id,
      observed_checkpoint_id: checkpoint.checkpoint_id,
      sender_stopped: true,
      provider_inspected: true,
    });
    expect(retries).toBe(0);
    await page.getByRole("button", { name: "Retry publication" }).click();
    expect(retries).toBe(1);
  });
});
