import { expect, test } from "@playwright/test";
import { withWorkspace } from "./helpers";

async function fixture(
  page: any,
  context: any,
  write: any,
  initial: "failed" | "uncertain",
) {
  const brand = await write("/brands", { name: "X recovery fixture" });
  const channel = await write("/channels", {
    brand_id: brand.id,
    provider: "manual",
    name: "Fixture X",
    timezone: "UTC",
  });
  const post = await write("/posts", {
    brand_id: brand.id,
    title: "Fixture post",
    text: "Local fixture",
    media_ids: [],
    variants: [],
  });
  const [publication] = await write(`/posts/${post.id}/schedule`, {
    version: post.version,
    channel_ids: [channel.id],
    scheduled_at: new Date(Date.now() + 86400000).toISOString(),
  });
  await context.addInitScript(
    (id: string) => localStorage.setItem("beweft.brand", id),
    brand.id,
  );
  let current = { ...publication, provider: "x", state: initial };
  await page.route("**/publications?*", (route: any) =>
    route.fulfill({ json: { data: [current] } }),
  );
  await page.route(`**/publications/${publication.id}/attempts`, (route: any) =>
    route.fulfill({ json: { data: [] } }),
  );
  return {
    publication,
    get: () => current,
    set: (next: any) => {
      current = next;
    },
  };
}

const status = (checkpoint: unknown) => ({
  session_expired: false,
  reset_allowed: false,
  reason: null,
  x_upload: checkpoint,
});
const checkpoint = (
  phase: "upload" | "post_started",
  reset_allowed: boolean,
) => ({
  phase,
  reset_allowed,
  attempt_id: crypto.randomUUID(),
  checkpoint_id: crypto.randomUUID(),
  reason: "Review this attempt.",
});

test("partial X thread displays the saved prefix and requires a complete inspected chain", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await fixture(page, context, write, "uncertain");
    f.set({
      ...f.get(),
      payload: {
        ...f.get().payload,
        options: { x_thread: { replies: ["First reply", "Second reply"] } },
      },
    });
    const cp = checkpoint("post_started", false);
    const thread = {
      phase: "request_started",
      confirmed_ids: ["1001"],
      next_index: 1,
      total: 3,
      remaining_texts: ["First reply", "Second reply"],
      attempt_id: cp.attempt_id,
      checkpoint_id: cp.checkpoint_id,
    };
    let submitted: any = null;
    await page.route(
      `**/publications/${f.publication.id}/provider-status`,
      (route) =>
        route.fulfill({
          json: {
            data: {
              ...status(cp),
              x_thread: thread,
              x_thread_retry_allowed: false,
            },
          },
        }),
    );
    await page.route(
      `**/publications/${f.publication.id}/reconcile`,
      (route) => {
        submitted = route.request().postDataJSON();
        return route.fulfill({
          json: {
            data: {
              ...f.get(),
              state: "published",
              external_id: "1001",
              external_url: "https://x.com/i/status/1001",
            },
          },
        });
      },
    );
    await page.goto("/#queue");
    await page.getByRole("button", { name: "Reconcile outcome" }).click();
    await expect(page.getByText("First reply", { exact: true })).toBeVisible();
    await expect(
      page.getByText(/Unconfirmed text may already be public/),
    ).toBeVisible();
    await expect(page.getByLabel("Post 1 ID (root)")).toHaveValue("1001");
    await expect(page.getByLabel("Post 1 ID (root)")).toHaveAttribute(
      "readonly",
      "",
    );
    await expect(
      page.getByRole("button", { name: "Reset X checkpoint" }),
    ).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: "Cancel publication" }),
    ).toHaveCount(0);
    await page.getByLabel("Post 2 ID").fill("1002");
    await page.getByLabel("Post 3 ID").fill("1003");
    await page
      .getByLabel("X verification note")
      .fill("Inspected the entire fixture chain.");
    await page
      .getByRole("checkbox", { name: /original sender has stopped/ })
      .check();
    await expect(
      page.getByRole("button", { name: "Record complete thread" }),
    ).toBeDisabled();
    await page
      .getByRole("checkbox", { name: /inspected the actual X reply chain/ })
      .check();
    await page.getByRole("button", { name: "Record complete thread" }).click();
    await expect.poll(() => submitted).not.toBeNull();
    expect(submitted).toMatchObject({
      resolution: "published",
      thread_ids: ["1001", "1002", "1003"],
      observed_attempt_id: cp.attempt_id,
      observed_checkpoint_id: cp.checkpoint_id,
      sender_stopped: true,
      provider_inspected: true,
    });
  });
});

test("root recovery sends a distinct attestation only for an explicit current root checkpoint", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await fixture(page, context, write, "uncertain");
    f.set({
      ...f.get(),
      payload: {
        ...f.get().payload,
        options: { x_thread: { replies: ["Reply"] } },
      },
    });
    const cp = checkpoint("post_started", false);
    let currentRoot = false;
    let submitted: any = null;
    await page.route(
      `**/publications/${f.publication.id}/provider-status`,
      (route) =>
        route.fulfill({
          json: {
            data: {
              ...status(cp),
              x_thread_retry_allowed: false,
              x_thread: {
                phase: "request_started",
                confirmed_ids: [],
                next_index: 0,
                total: 2,
                remaining_texts: ["Local fixture", "Reply"],
                attempt_id: cp.attempt_id,
                checkpoint_id: cp.checkpoint_id,
                root_request_started: currentRoot,
              },
            },
          },
        }),
    );
    await page.route(
      `**/publications/${f.publication.id}/reconcile`,
      (route) => {
        submitted = route.request().postDataJSON();
        return route.fulfill({
          json: { data: { ...f.get(), state: "failed" } },
        });
      },
    );
    await page.goto("/#queue");
    await page.getByRole("button", { name: "Reconcile outcome" }).click();
    await expect(page.getByText("Reply", { exact: true })).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Record root not published" }),
    ).toHaveCount(0);
    currentRoot = true;
    await page.getByRole("button", { name: "Check X recovery status" }).click();
    await expect(
      page.getByRole("button", { name: "Record root not published" }),
    ).toBeVisible();
    await page
      .getByLabel("X verification note")
      .fill("Verified remote operation ended.");
    await page
      .getByRole("checkbox", { name: /original sender has stopped/ })
      .check();
    await page
      .getByRole("checkbox", {
        name: /root operation definitively ended without creating a post/,
      })
      .check();
    await page
      .getByRole("button", { name: "Record root not published" })
      .click();
    await expect.poll(() => submitted).not.toBeNull();
    expect(submitted).toMatchObject({
      resolution: "not_published",
      observed_attempt_id: cp.attempt_id,
      observed_checkpoint_id: cp.checkpoint_id,
      sender_stopped: true,
      provider_inspected: true,
      thread_root_definitively_ended: true,
    });
  });
});

test("X composer saves ordered text replies on the channel variant", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "X composer fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "X fixture",
      timezone: "UTC",
    });
    await context.addInitScript(
      (id: string) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.route("**/channels?*", (route) =>
      route.fulfill({
        json: {
          data: [
            {
              ...channel,
              provider: "x",
              connection_status: "connected",
              capabilities: {
                ...channel.capabilities,
                formats: ["text", "image", "video", "thread"],
              },
            },
          ],
        },
      }),
    );
    await page.goto("/#queue");
    await page
      .getByRole("button", { name: "Create post", exact: true })
      .click();
    await page.getByLabel("Post text", { exact: true }).fill("Root fixture");
    await page.getByRole("button", { name: /X fixture$/ }).click();
    await page.getByRole("tab", { name: "X fixture" }).click();
    await expect(
      page.getByText(/Attachments are sent with the first post only/),
    ).toBeVisible();
    await page.getByRole("button", { name: "Add reply" }).click();
    await page.getByLabel("Reply 1").fill("Second in thread");
    await page.getByRole("button", { name: "Add reply" }).click();
    await page.getByLabel("Reply 2").fill("First in thread");
    await page.getByRole("button", { name: "Move reply 2 up" }).click();
    await page.getByRole("button", { name: "Save draft", exact: true }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    const response = await context.request.get(
      `/api/v1/posts?brand_id=${brand.id}`,
    );
    expect(
      (await response.json()).data[0].variants[0].options.x_thread,
    ).toEqual({ replies: ["First in thread", "Second in thread"] });
  });
});

test("saved thread options cannot be scheduled through a server without thread support", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Older X server fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Old X",
      timezone: "UTC",
    });
    const draft = await write("/posts", {
      brand_id: brand.id,
      title: "Saved thread",
      text: "Root",
      media_ids: [],
      variants: [
        {
          channel_id: channel.id,
          options: { x_thread: { replies: ["Keep this reply"] } },
        },
      ],
    });
    await context.addInitScript(
      (id: string) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.route("**/channels?*", (route) =>
      route.fulfill({
        json: {
          data: [
            {
              ...channel,
              provider: "x",
              connection_status: "connected",
              capabilities: { formats: ["text", "image", "video"] },
            },
          ],
        },
      }),
    );
    let schedules = 0;
    await page.route(`**/posts/${draft.id}/schedule`, (route) => {
      schedules++;
      return route.abort();
    });
    await page.goto("/#drafts");
    await page.getByRole("button", { name: "Edit draft" }).click();
    await page.getByRole("tab", { name: "Old X" }).click();
    await expect(page.getByText(/Saved replies are retained/)).toBeVisible();
    await expect(page.getByRole("button", { name: "Add reply" })).toHaveCount(
      0,
    );
    await page.getByLabel("When should this go out?").selectOption("queue");
    await page.getByRole("button", { name: "Add to queue" }).click();
    await expect(
      page.getByText(/Update the server before scheduling these saved replies/),
    ).toBeVisible();
    expect(schedules).toBe(0);
    await page.getByLabel("When should this go out?").selectOption("draft");
    await page.getByRole("button", { name: "Save draft" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    const saved = (
      await (await context.request.get(`/api/v1/posts/${draft.id}`)).json()
    ).data;
    expect(saved.variants[0].options.x_thread).toEqual({
      replies: ["Keep this reply"],
    });
  });
});

test("interrupted upload reset uses current IDs and never retries automatically", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await fixture(page, context, write, "failed");
    let current: any = checkpoint("upload", true);
    const first = current,
      next = checkpoint("upload", true),
      resets: any[] = [];
    let retries = 0;
    await page.route(
      `**/publications/${f.publication.id}/provider-status`,
      (route) => route.fulfill({ json: { data: status(current) } }),
    );
    await page.route(
      `**/publications/${f.publication.id}/reset-x-upload`,
      (route) => {
        resets.push(route.request().postDataJSON());
        if (resets.length === 1) {
          current = next;
          return route.fulfill({
            status: 409,
            json: {
              error: { code: "conflict", message: "Checkpoint changed." },
            },
          });
        }
        current = null;
        return route.fulfill({
          json: {
            data: {
              state: "failed",
              session_reset: true,
              next_action: "Review, then explicitly retry.",
            },
          },
        });
      },
    );
    await page.route(`**/publications/${f.publication.id}/retry`, (route) => {
      retries++;
      return route.abort();
    });
    await page.goto("/#queue");
    await page.getByRole("button", { name: "View details" }).click();
    await expect(page.getByText(first.checkpoint_id)).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Retry publication" }),
    ).toBeDisabled();
    const stop = page.getByRole("checkbox", {
      name: /I confirmed the original sender has stopped/,
    });
    const restart = page.getByRole("checkbox", {
      name: /I want to reset this checkpoint/,
    });
    await expect(
      page.getByRole("button", { name: "Reset X checkpoint" }),
    ).toBeDisabled();
    await stop.check();
    await restart.check();
    await page.getByRole("button", { name: "Reset X checkpoint" }).click();
    await expect(page.getByText(next.checkpoint_id)).toBeVisible();
    await expect(stop).not.toBeChecked();
    await expect(restart).not.toBeChecked();
    await stop.check();
    await restart.check();
    await page.getByRole("button", { name: "Reset X checkpoint" }).click();
    await expect(
      page.getByText("Review, then explicitly retry."),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Retry publication" }),
    ).toBeEnabled();
    expect(resets).toEqual(
      [first, next].map((cp) => ({
        observed_attempt_id: cp.attempt_id,
        observed_checkpoint_id: cp.checkpoint_id,
        sender_stopped: true,
        confirm_restart: true,
      })),
    );
    expect(retries).toBe(0);
  });
});

test("a started post requires fresh exact-ID inspection before reset", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await fixture(page, context, write, "uncertain");
    let current: any = checkpoint("post_started", false);
    const first = current,
      next = checkpoint("post_started", false),
      resolutions: any[] = [],
      resets: any[] = [];
    let retries = 0;
    await page.route(
      `**/publications/${f.publication.id}/provider-status`,
      (route) => route.fulfill({ json: { data: status(current) } }),
    );
    await page.route(
      `**/publications/${f.publication.id}/reconcile`,
      (route) => {
        resolutions.push(route.request().postDataJSON());
        if (resolutions.length === 1) {
          current = next;
          return route.fulfill({
            status: 409,
            json: { error: { code: "conflict", message: "Attempt changed." } },
          });
        }
        current = { ...next, reset_allowed: true };
        const updated = {
          ...f.get(),
          state: "failed",
          updated_at: new Date().toISOString(),
        };
        f.set(updated);
        return route.fulfill({ json: { data: updated } });
      },
    );
    await page.route(
      `**/publications/${f.publication.id}/reset-x-upload`,
      (route) => {
        resets.push(route.request().postDataJSON());
        current = null;
        return route.fulfill({
          json: {
            data: {
              state: "failed",
              session_reset: true,
              next_action: "Reset complete; retry separately.",
            },
          },
        });
      },
    );
    await page.route(`**/publications/${f.publication.id}/retry`, (route) => {
      retries++;
      return route.abort();
    });
    await page.goto("/#queue");
    await page.getByRole("button", { name: "Reconcile outcome" }).click();
    await expect(page.getByText(first.checkpoint_id)).toBeVisible();
    async function inspect() {
      await page.getByLabel("Verified X outcome").selectOption("not_published");
      await page
        .getByLabel("X verification note")
        .fill("Inspected X for this attempt.");
      await expect(
        page.getByRole("button", { name: "Record X outcome" }),
      ).toBeDisabled();
      await page
        .getByRole("checkbox", {
          name: /I confirmed the original sender has stopped/,
        })
        .check();
      await page
        .getByRole("checkbox", { name: /I inspected X and verified/ })
        .check();
      await page.getByRole("button", { name: "Record X outcome" }).click();
    }
    await inspect();
    await expect(page.getByText(next.checkpoint_id)).toBeVisible();
    await expect(page.getByLabel("Verified X outcome")).toHaveValue("");
    await inspect();
    await expect(
      page.getByRole("button", { name: "Reset X checkpoint" }),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Retry publication" }),
    ).toBeDisabled();
    await page
      .getByRole("checkbox", {
        name: /I confirmed the original sender has stopped/,
      })
      .check();
    await page
      .getByRole("checkbox", {
        name: /I inspected X for this attempt before resetting/,
      })
      .check();
    await page
      .getByRole("checkbox", { name: /I want to reset this checkpoint/ })
      .check();
    await page.getByRole("button", { name: "Reset X checkpoint" }).click();
    await expect(
      page.getByText("Reset complete; retry separately."),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Retry publication" }),
    ).toBeEnabled();
    expect(
      resolutions.map((body) => [
        body.observed_attempt_id,
        body.observed_checkpoint_id,
        body.sender_stopped,
        body.provider_inspected,
      ]),
    ).toEqual(
      [first, next].map((cp) => [cp.attempt_id, cp.checkpoint_id, true, true]),
    );
    expect(resets).toEqual([
      {
        observed_attempt_id: next.attempt_id,
        observed_checkpoint_id: next.checkpoint_id,
        sender_stopped: true,
        confirm_restart: true,
        provider_inspected: true,
      },
    ]);
    expect(retries).toBe(0);
  });
});

test("an older server leaves X recovery unknown instead of enabling retry", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await fixture(page, context, write, "failed");
    await page.route(
      `**/publications/${f.publication.id}/provider-status`,
      (route) =>
        route.fulfill({
          json: {
            data: {
              session_expired: false,
              reset_allowed: false,
              reason: null,
            },
          },
        }),
    );
    await page.goto("/#queue");
    await page.getByRole("button", { name: "View details" }).click();
    await expect(
      page.getByText(/server cannot report X checkpoints/),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Retry publication" }),
    ).toBeDisabled();
    await expect(
      page.getByRole("button", { name: "Reset X checkpoint" }),
    ).toHaveCount(0);
  });
});

test("an unavailable status blocks X retry until a fresh read succeeds", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await fixture(page, context, write, "failed");
    let unavailable = true;
    await page.route(
      `**/publications/${f.publication.id}/provider-status`,
      (route) =>
        unavailable
          ? route.fulfill({
              status: 503,
              json: { error: { code: "busy", message: "Status unavailable." } },
            })
          : route.fulfill({ json: { data: status(null) } }),
    );
    await page.goto("/#queue");
    await page.getByRole("button", { name: "View details" }).click();
    await expect(page.getByText("Status unavailable.")).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Retry publication" }),
    ).toBeDisabled();
    unavailable = false;
    await page.getByRole("button", { name: "Check X recovery status" }).click();
    await expect(
      page.getByRole("button", { name: "Retry publication" }),
    ).toBeEnabled();
  });
});
