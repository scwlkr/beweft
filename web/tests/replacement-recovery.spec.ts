import { execFileSync, spawn } from "node:child_process";
import { expect, test, type APIResponse } from "@playwright/test";
import { deferred, withWorkspace } from "./helpers";

function sql(statement: string) {
  const database = process.env.BEWEFT_TEST_DB_NAME ?? "";
  if (!/^beweft_e2e_[a-f0-9]{12}$/.test(database))
    throw new Error("Requires the isolated test database");
  return execFileSync(
    "docker",
    [
      "exec",
      process.env.BEWEFT_TEST_POSTGRES_CONTAINER ?? "beweft-dev-postgres",
      "psql",
      "-U",
      process.env.BEWEFT_TEST_DB_USER ?? "beweft",
      "-d",
      database,
      "-v",
      "ON_ERROR_STOP=1",
      "-Atq",
      "-c",
      statement,
    ],
    { encoding: "utf8" },
  ).trim();
}
async function holdPost(id: string) {
  if (!/^[a-f0-9-]{36}$/.test(id)) throw new Error("Expected a fixture UUID");
  const database = process.env.BEWEFT_TEST_DB_NAME ?? "";
  if (!/^beweft_e2e_[a-f0-9]{12}$/.test(database))
    throw new Error("Requires the isolated test database");
  const child = spawn("docker", [
    "exec",
    "-i",
    process.env.BEWEFT_TEST_POSTGRES_CONTAINER ?? "beweft-dev-postgres",
    "psql",
    "-U",
    process.env.BEWEFT_TEST_DB_USER ?? "beweft",
    "-d",
    database,
    "-v",
    "ON_ERROR_STOP=1",
    "-Atq",
  ]);
  const ready = deferred();
  let output = "";
  child.stdout.on("data", (chunk: Buffer) => {
    output += chunk.toString();
    if (output.includes("FIXTURE_LOCKED")) ready.resolve();
  });
  child.stdin.write(
    `BEGIN; SELECT id FROM posts WHERE id='${id}' FOR UPDATE; SELECT 'FIXTURE_LOCKED';\n`,
  );
  await Promise.race([
    ready.promise,
    new Promise<never>((_, reject) =>
      child.once("exit", () => reject(new Error("Fixture lock exited"))),
    ),
  ]);
  return async () => {
    if (child.exitCode !== null) return;
    child.stdin.end("COMMIT;\n");
    await new Promise<void>((resolve) => child.once("exit", () => resolve()));
  };
}
async function publication(
  context: import("@playwright/test").BrowserContext,
  brand: string,
  id: string,
) {
  const response = await context.request.get(
    `/api/v1/publications?brand_id=${brand}`,
  );
  expect(response.ok()).toBe(true);
  return (await response.json()).data.find(
    (row: { id: string }) => row.id === id,
  );
}

test("a real worker claim followed by a lost replacement 409 stays blocked while sending", async () => {
  test.skip(
    Number(process.env.BEWEFT_FIXTURE_DELAY_MS ?? 0) < 15000,
    "Run with BEWEFT_FIXTURE_DELAY_MS=30000 to observe the owned fixture worker claim.",
  );
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Worker claim recovery" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "fixture",
      name: "Delayed fixture",
      timezone: "UTC",
    });
    const post = await write("/posts", {
      brand_id: brand.id,
      title: "Claimed original",
      text: "Fixture only",
      variants: [{ channel_id: channel.id }],
    });
    const [original] = await write(`/posts/${post.id}/schedule`, {
      version: post.version,
      channel_ids: [channel.id],
      scheduled_at: new Date(Date.now() + 86400000).toISOString(),
      media_versions: {},
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    let responseStatus = 0;
    await page.route(
      `**/api/v1/publications/${original.id}/replace`,
      async (route) => {
        sql(
          `UPDATE publications SET scheduled_at=now() WHERE id='${original.id}'`,
        );
        await expect
          .poll(
            async () =>
              (await publication(context, brand.id, original.id)).state,
            { timeout: 10000 },
          )
          .toBe("sending");
        const response = await route.fetch();
        responseStatus = response.status();
        await route.abort("failed");
      },
    );
    await page.goto("/#queue");
    await page.getByRole("button", { name: "View details" }).click();
    await page.getByRole("button", { name: "Compare and replace" }).click();
    await page.getByRole("button", { name: "Replace queued content" }).click();
    await expect.poll(() => responseStatus, { timeout: 10000 }).toBe(409);
    await expect(
      page.getByText(/queued replacement has an unconfirmed outcome/i),
    ).toBeVisible();
    await page.getByRole("button", { name: "Close dialog" }).click();
    await page.getByRole("button", { name: "Check replacement" }).click();
    await expect(
      page.getByText(/original is sending.*scheduling stays blocked/i),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Finish review" }),
    ).toHaveCount(0);
    await page.reload();
    await expect(
      page.getByText(/queued replacement has an unconfirmed outcome/i),
    ).toBeVisible();
    const status = await context.request.get(
      `/api/v1/publications/${original.id}/replacement-status`,
    );
    expect((await status.json()).data).toMatchObject({
      original: { id: original.id, state: "sending" },
      replacement: null,
    });
  });
});

test("a pending replacement remains blocked through a real retry and needs an explicit terminal review", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Pending replace and retry" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Retry fixture",
      timezone: "UTC",
    });
    const post = await write("/posts", {
      brand_id: brand.id,
      title: "Pending original",
      text: "Fixture only",
      variants: [{ channel_id: channel.id }],
    });
    const [original] = await write(`/posts/${post.id}/schedule`, {
      version: post.version,
      channel_ids: [channel.id],
      scheduled_at: new Date(Date.now() + 86400000).toISOString(),
      media_versions: {},
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    const release = await holdPost(post.id);
    let held = true;
    let pending: Promise<APIResponse> | undefined;
    try {
      await page.route(
        `**/api/v1/publications/${original.id}/replace`,
        async (route) => {
          pending = context.request.post(
            `/api/v1/publications/${original.id}/replace`,
            {
              data: route.request().postDataJSON(),
              headers: {
                "X-Beweft-Request": "1",
                Origin: process.env.BEWEFT_TEST_URL!,
              },
            },
          );
          await expect
            .poll(
              () =>
                Number(
                  sql(
                    "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'SELECT % FROM posts WHERE id=% FOR UPDATE'",
                  ),
                ),
              { timeout: 10000 },
            )
            .toBeGreaterThan(0);
          await route.abort("failed");
        },
      );
      await page.goto("/#queue");
      await page.getByRole("button", { name: "View details" }).click();
      await page.getByRole("button", { name: "Compare and replace" }).click();
      await page
        .getByRole("button", { name: "Replace queued content" })
        .click();
      await expect(
        page.getByText(/queued replacement has an unconfirmed outcome/i),
      ).toBeVisible();
      await page.getByRole("button", { name: "Close dialog" }).click();
      await page.getByRole("button", { name: "Check replacement" }).click();
      await expect(
        page.getByText(/original is queued.*scheduling stays blocked/i),
      ).toBeVisible();
      const pause = await context.request.patch(
        `/api/v1/channels/${channel.id}`,
        {
          data: { paused: true },
          headers: {
            "X-Beweft-Request": "1",
            Origin: process.env.BEWEFT_TEST_URL!,
          },
        },
      );
      expect(pause.ok()).toBe(true);
      // Simulate an uncertain fixture attempt, then use the real reconciliation and retry actions.
      sql(
        `UPDATE publications SET state='uncertain',last_error='fixture outcome unknown' WHERE id='${original.id}'`,
      );
      await page.getByRole("button", { name: "Check replacement" }).click();
      await expect(
        page.getByText(/original is uncertain.*scheduling stays blocked/i),
      ).toBeVisible();
      await write(`/publications/${original.id}/reconcile`, {
        resolution: "not_published",
        note: "Isolated fixture has no external delivery",
      });
      await page.getByRole("button", { name: "Check replacement" }).click();
      await expect(
        page.getByText(/original is failed.*scheduling stays blocked/i),
      ).toBeVisible();
      await write(`/publications/${original.id}/retry`, {});
      expect((await publication(context, brand.id, original.id)).state).toBe(
        "queued",
      );
      await page.getByRole("button", { name: "Check replacement" }).click();
      await expect(
        page.getByText(/original is queued.*scheduling stays blocked/i),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Finish review" }),
      ).toHaveCount(0);
      await release();
      held = false;
      expect(pending).toBeDefined();
      expect((await pending!).status()).toBe(409);
      await write(`/publications/${original.id}/cancel`, {});
      await page.getByRole("button", { name: "Check replacement" }).click();
      await expect(
        page.getByText(/original reached a terminal state/i),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Finish review" }),
      ).toBeDisabled();
      await page
        .getByRole("checkbox", { name: /I reviewed these outcomes/ })
        .check();
      await page.getByRole("button", { name: "Finish review" }).click();
      await expect(
        page.getByRole("button", { name: "Check replacement" }),
      ).toHaveCount(0);
      const rows = (
        await (
          await context.request.get(`/api/v1/publications?brand_id=${brand.id}`)
        ).json()
      ).data;
      expect(rows).toHaveLength(1);
      expect(rows[0].state).toBe("canceled");
    } finally {
      if (held) await release();
      if (pending) await pending.catch(() => undefined);
    }
  });
});

test("a different audit-linked replacement requires explicit review", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", {
      name: "Different replacement fixture",
    });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Different channel",
      timezone: "UTC",
    });
    const post = await write("/posts", {
      brand_id: brand.id,
      title: "Different revision",
      text: "Original",
      variants: [{ channel_id: channel.id }],
    });
    const [original] = await write(`/posts/${post.id}/schedule`, {
      version: post.version,
      channel_ids: [channel.id],
      scheduled_at: new Date(Date.now() + 86400000).toISOString(),
      media_versions: {},
    });
    const firstEdit = await context.request.patch(`/api/v1/posts/${post.id}`, {
      data: { version: post.version, text: "Version two" },
      headers: {
        "X-Beweft-Request": "1",
        Origin: process.env.BEWEFT_TEST_URL!,
      },
    });
    expect(firstEdit.ok()).toBe(true);
    const secondVersion = (await firstEdit.json()).data.version;
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    let linkedId = "";
    await page.route(
      `**/api/v1/publications/${original.id}/replace`,
      async (route) => {
        const nextEdit = await context.request.patch(
          `/api/v1/posts/${post.id}`,
          {
            data: { version: secondVersion, text: "Version three" },
            headers: {
              "X-Beweft-Request": "1",
              Origin: process.env.BEWEFT_TEST_URL!,
            },
          },
        );
        expect(nextEdit.ok()).toBe(true);
        const next = (await nextEdit.json()).data;
        const response = await context.request.post(
          `/api/v1/publications/${original.id}/replace`,
          {
            data: {
              post_version: next.version,
              publication_post_version: original.post_version,
              scheduled_at: original.scheduled_at,
            },
            headers: {
              "X-Beweft-Request": "1",
              Origin: process.env.BEWEFT_TEST_URL!,
            },
          },
        );
        expect(response.ok(), await response.text()).toBe(true);
        linkedId = (await response.json()).data.publication.id;
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
    await expect.poll(() => linkedId).not.toBe("");
    await page.getByRole("button", { name: "Close dialog" }).click();
    await page.getByRole("button", { name: "Check replacement" }).click();
    await expect(
      page.getByText(/linked replacement differs from the saved request/i),
    ).toBeVisible();
    await expect(page.getByText(linkedId)).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Finish review" }),
    ).toBeDisabled();
    await page
      .getByRole("checkbox", { name: /I reviewed these outcomes/ })
      .check();
    await page.getByRole("button", { name: "Finish review" }).click();
    await expect(
      page.getByRole("button", { name: "Check replacement" }),
    ).toHaveCount(0);
    const rows = (
      await (
        await context.request.get(`/api/v1/publications?brand_id=${brand.id}`)
      ).json()
    ).data;
    expect(rows).toHaveLength(2);
    expect(
      rows.find((row: { id: string }) => row.id === linkedId).post_version,
    ).toBe(secondVersion + 1);
  });
});

test("a late status read cannot clear another brand's pending replacement", async () => {
  await withWorkspace(async (page, context, write) => {
    const oldBrand = await write("/brands", { name: "Pending original brand" });
    const nextBrand = await write("/brands", { name: "Next workspace" });
    const channel = await write("/channels", {
      brand_id: oldBrand.id,
      provider: "manual",
      name: "Old channel",
      timezone: "UTC",
    });
    const post = await write("/posts", {
      brand_id: oldBrand.id,
      title: "Pending status",
      text: "Fixture",
      variants: [{ channel_id: channel.id }],
    });
    const [original] = await write(`/posts/${post.id}/schedule`, {
      version: post.version,
      channel_ids: [channel.id],
      scheduled_at: new Date(Date.now() + 86400000).toISOString(),
      media_versions: {},
    });
    const owner = (await (await context.request.get("/api/v1/auth/me")).json())
      .data;
    const key = `beweft.pending-schedule:${owner.id}:${oldBrand.id}:replacement`;
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      oldBrand.id,
    );
    await page.route(
      `**/api/v1/publications/${original.id}/replace`,
      async (route) => {
        const result = await route.fetch();
        expect(result.ok()).toBe(true);
        await route.abort("failed");
      },
    );
    const started = deferred(),
      release = deferred();
    await page.route(
      `**/api/v1/publications/${original.id}/replacement-status`,
      async (route) => {
        const result = await route.fetch();
        started.resolve();
        await release.promise;
        await route.fulfill({ response: result });
      },
    );
    try {
      await page.goto("/#queue");
      await page.getByRole("button", { name: "View details" }).click();
      await page.getByRole("button", { name: "Compare and replace" }).click();
      await page
        .getByRole("button", { name: "Replace queued content" })
        .click();
      await expect(
        page.getByText(/queued replacement has an unconfirmed outcome/i),
      ).toBeVisible();
      await page.getByRole("button", { name: "Close dialog" }).click();
      await page.getByRole("button", { name: "Check replacement" }).click();
      await started.promise;
      await page.getByLabel("Current brand").selectOption(nextBrand.id);
      release.resolve();
      await expect(page.getByLabel("Current brand")).toHaveValue(nextBrand.id);
      expect(
        await page.evaluate(
          (storageKey) => localStorage.getItem(storageKey),
          key,
        ),
      ).not.toBeNull();
      await page.unroute(
        `**/api/v1/publications/${original.id}/replacement-status`,
      );
      await page.getByLabel("Current brand").selectOption(oldBrand.id);
      await page.getByRole("button", { name: "Check replacement" }).click();
      await expect(
        page.getByRole("button", { name: "Check replacement" }),
      ).toHaveCount(0);
      expect(
        await page.evaluate(
          (storageKey) => localStorage.getItem(storageKey),
          key,
        ),
      ).toBeNull();
    } finally {
      release.resolve();
    }
  });
});
