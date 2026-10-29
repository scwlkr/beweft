import { execFileSync, spawn } from "node:child_process";
import { expect, test, type BrowserContext, type Page } from "@playwright/test";
import { deferred, withWorkspace } from "./helpers";

const pixel = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aL1sAAAAASUVORK5CYII=",
  "base64",
);
const container =
  process.env.BEWEFT_TEST_POSTGRES_CONTAINER ?? "beweft-dev-postgres";
const dbUser = process.env.BEWEFT_TEST_DB_USER ?? "beweft";
function database() {
  const db = process.env.BEWEFT_TEST_DB_NAME ?? "";
  if (!/^beweft_e2e_[a-f0-9]{12}$/.test(db))
    throw new Error("Requires a disposable database");
  return db;
}
function sql(statement: string) {
  return execFileSync(
    "docker",
    [
      "exec",
      container,
      "psql",
      "-U",
      dbUser,
      "-d",
      database(),
      "-v",
      "ON_ERROR_STOP=1",
      "-Atq",
      "-c",
      statement,
    ],
    { encoding: "utf8" },
  ).trim();
}
async function holdLock(statement: string) {
  const process = spawn("docker", [
    "exec",
    "-i",
    container,
    "psql",
    "-U",
    dbUser,
    "-d",
    database(),
    "-v",
    "ON_ERROR_STOP=1",
    "-Atq",
  ]);
  const ready = deferred();
  let output = "";
  process.stdout.on("data", (chunk: Buffer) => {
    output += chunk.toString();
    if (output.includes("FIXTURE_LOCKED")) ready.resolve();
  });
  process.stdin.write(`BEGIN; ${statement}; SELECT 'FIXTURE_LOCKED';\n`);
  await Promise.race([
    ready.promise,
    new Promise<never>((_, reject) =>
      process.once("exit", () => reject(new Error("Fixture lock exited"))),
    ),
  ]);
  return async () => {
    if (process.exitCode !== null) return;
    process.stdin.end("COMMIT;\n");
    await new Promise<void>((resolve) => process.once("exit", () => resolve()));
  };
}
async function waitForLock(fragment: string) {
  if (!/^[A-Za-z _]+$/.test(fragment))
    throw new Error("Invalid fixture query fragment");
  await expect
    .poll(
      () =>
        Number(
          sql(
            `SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE '%${fragment}%'`,
          ),
        ),
      { timeout: 10000 },
    )
    .toBeGreaterThan(0);
}
async function upload(context: BrowserContext, brandId: string, name: string) {
  const response = await context.request.post(
    `/api/v1/media?brand_id=${brandId}`,
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
async function read(context: BrowserContext, path: string) {
  const response = await context.request.get(`/api/v1${path}`);
  expect(response.ok(), await response.text()).toBe(true);
  return (await response.json()).data;
}
async function patch(context: BrowserContext, path: string, body: unknown) {
  const response = await context.request.patch(`/api/v1${path}`, {
    data: body,
    headers: { "X-Beweft-Request": "1", Origin: process.env.BEWEFT_TEST_URL! },
  });
  expect(response.ok(), await response.text()).toBe(true);
  return (await response.json()).data;
}
async function setup(
  page: Page,
  context: BrowserContext,
  write: (path: string, body: unknown) => Promise<any>,
  withMedia = false,
) {
  const brand = await write("/brands", {
    name: `Real schedule ${crypto.randomUUID()}`,
  });
  const channel = await write("/channels", {
    brand_id: brand.id,
    provider: "manual",
    name: "Real recovery channel",
    timezone: "UTC",
  });
  const media = withMedia
    ? await upload(context, brand.id, "receipt.png")
    : null;
  const post = await write("/posts", {
    brand_id: brand.id,
    title: "Real recovery draft",
    text: "Approved draft",
    media_ids: media ? [media.id] : [],
    variants: [{ channel_id: channel.id }],
  });
  const owner = await read(context, "/auth/me");
  const key = `beweft.pending-schedule:${owner.id}:${brand.id}`;
  await context.addInitScript(
    (id) => localStorage.setItem("beweft.brand", id),
    brand.id,
  );
  let patches = 0;
  page.on("request", (request) => {
    if (
      request.method() === "PATCH" &&
      new URL(request.url()).pathname === `/api/v1/posts/${post.id}`
    )
      patches++;
  });
  return { brand, channel, media, post, owner, key, patches: () => patches };
}
function localTime(offsetMs: number, now = Date.now()) {
  const date = new Date(now + offsetMs);
  const value = new Date(date.getTime() - date.getTimezoneOffset() * 60000)
    .toISOString()
    .slice(0, 19);
  // Chromium normalizes a zero seconds component away before Playwright
  // verifies a datetime-local fill.
  return value.endsWith(":00") ? value.slice(0, 16) : value;
}
async function start(page: Page, date = localTime(7 * 86400_000)) {
  await page.goto("/#drafts");
  await page.getByRole("button", { name: "Edit draft", exact: true }).click();
  await page.getByLabel("When should this go out?").selectOption("exact");
  await page.getByLabel(/Date and time/).fill(date);
  await expect(
    page.getByRole("button", { name: "Schedule post" }),
  ).toBeEnabled();
  await page.getByRole("button", { name: "Schedule post" }).click();
}
async function pending(page: Page, key: string) {
  return page.evaluate((storageKey) => {
    const raw = localStorage.getItem(storageKey);
    return raw === null ? null : JSON.parse(raw);
  }, key);
}
function routeFor(postId: string) {
  return new RegExp(`/api/v1/posts/${postId}/schedule\\?view=summary$`);
}

test("real committed response loss survives reload and same-ID retry without another revision", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    const otherBrand = await write("/brands", {
      name: "Schedule recovery other brand",
    });
    const committed = deferred();
    const requests: any[] = [];
    let originalRows: any[] = [];
    let beforeWire: any;
    await page.route(routeFor(f.post.id), async (route) => {
      requests.push(route.request().postDataJSON());
      if (requests.length !== 1) return route.continue();
      beforeWire = await pending(page, f.key);
      const response = await route.fetch();
      expect(response.ok(), await response.text()).toBe(true);
      originalRows = (await response.json()).data;
      committed.resolve();
      await route.abort("failed").catch(() => {});
    });
    await start(page);
    await committed.promise;
    await expect(
      page.getByRole("button", { name: "Retry original schedule" }),
    ).toBeEnabled();
    const saved = await pending(page, f.key);
    expect(beforeWire.uncertain).toBe(true);
    expect(beforeWire.id).toBe(requests[0].request_id);
    expect(beforeWire.post.version).toBe(requests[0].version);
    expect(beforeWire.channel_ids).toEqual(requests[0].channel_ids);
    expect(beforeWire.scheduled_at).toBe(requests[0].scheduled_at);
    expect(beforeWire.media_versions).toEqual(requests[0].media_versions);
    expect(saved.owner_id).toBe(f.owner.id);
    expect(saved.brand_id).toBe(f.brand.id);
    expect(saved.media_versions).toEqual({});
    expect(saved.channel_ids).toEqual([f.channel.id]);
    expect(saved.post.version).toBe(requests[0].version);
    const receipt = await read(context, `/schedule-requests/${saved.id}`);
    expect(receipt.request.media_versions).toEqual({});
    expect(receipt.publications.map((row: any) => row.id)).toEqual(
      originalRows.map((row) => row.id),
    );
    await page.getByRole("button", { name: "Close", exact: true }).click();
    await page.getByLabel("Current brand").selectOption(otherBrand.id);
    await expect(
      page.getByRole("button", { name: "Review pending schedule" }),
    ).toHaveCount(0);
    await page.getByLabel("Current brand").selectOption(f.brand.id);
    await expect(
      page.getByRole("button", { name: "Review pending schedule" }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Sign out" }).click();
    await page.getByLabel("Email address").fill(process.env.BEWEFT_TEST_EMAIL!);
    await page
      .getByLabel("Password", { exact: true })
      .fill(process.env.BEWEFT_TEST_PASSWORD!);
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(
      page.getByRole("button", { name: "Review pending schedule" }),
    ).toBeVisible();
    await page.reload();
    await page.getByRole("button", { name: "Review pending schedule" }).click();
    await expect(page.getByLabel("Post text", { exact: true })).toBeDisabled();
    await page.getByRole("button", { name: "Retry original schedule" }).click();
    await expect(
      page.getByText(/The original request is confirmed/),
    ).toBeVisible();
    expect(requests).toHaveLength(2);
    expect(requests[1]).toEqual(requests[0]);
    expect((await read(context, `/posts/${f.post.id}`)).version).toBe(
      saved.post.version,
    );
    expect(
      (await read(context, `/publications?brand_id=${f.brand.id}`)).map(
        (row: any) => row.id,
      ),
    ).toEqual(originalRows.map((row) => row.id));
    expect(f.patches()).toBe(1);
  });
});

test("real in-flight 404 and in-progress retry retain the same request until its one commit", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    const release = await holdLock(
      `SELECT pg_advisory_xact_lock(hashtextextended('${f.channel.id}',0))`,
    );
    const requests: any[] = [];
    page.on("request", (request) => {
      if (
        request.method() === "POST" &&
        routeFor(f.post.id).test(request.url())
      )
        requests.push(request.postDataJSON());
    });
    try {
      await start(page);
      await waitForLock("pg_advisory_xact_lock");
      const saved = await pending(page, f.key);
      expect(saved.uncertain).toBe(true);
      expect(
        (
          await context.request.get(`/api/v1/schedule-requests/${saved.id}`)
        ).status(),
      ).toBe(404);
      await page.getByRole("button", { name: "Close", exact: true }).click();
      await page
        .getByRole("button", { name: "Review pending schedule" })
        .click();
      await page
        .getByRole("button", { name: "Check scheduling outcome" })
        .click();
      await expect(
        page.getByText(/No request record is visible yet/),
      ).toBeVisible();
      await page
        .getByRole("button", { name: "Retry original schedule" })
        .click();
      await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
        "still in progress",
      );
      expect((await pending(page, f.key)).uncertain).toBe(true);
      expect(requests[1]).toEqual(requests[0]);
      await release();
      await expect
        .poll(async () =>
          (
            await context.request.get(`/api/v1/schedule-requests/${saved.id}`)
          ).status(),
        )
        .toBe(200);
      await page
        .getByRole("button", { name: "Retry original schedule" })
        .click();
      await expect(
        page.getByText(/The original request is confirmed/),
      ).toBeVisible();
      expect(requests[2]).toEqual(requests[0]);
      const receipt = await read(context, `/schedule-requests/${saved.id}`);
      expect(
        (await read(context, `/publications?brand_id=${f.brand.id}`)).map(
          (row: any) => row.id,
        ),
      ).toEqual(receipt.publications.map((row: any) => row.id));
      expect((await read(context, `/posts/${f.post.id}`)).version).toBe(
        saved.post.version,
      );
      expect(f.patches()).toBe(1);
    } finally {
      await release();
    }
  });
});

test("real retry returns the canceled original after replacement and its time has passed", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    const committed = deferred();
    const requests: any[] = [];
    let original: any;
    await page.route(routeFor(f.post.id), async (route) => {
      requests.push(route.request().postDataJSON());
      if (requests.length !== 1) return route.continue();
      const response = await route.fetch();
      expect(response.ok(), await response.text()).toBe(true);
      original = (await response.json()).data[0];
      committed.resolve();
      await route.abort("failed").catch(() => {});
    });
    await start(page, localTime(20_000));
    await committed.promise;
    await expect(
      page.getByRole("button", { name: "Retry original schedule" }),
    ).toBeEnabled();
    const saved = await pending(page, f.key);
    const revised = await patch(context, `/posts/${f.post.id}`, {
      version: saved.post.version,
      text: "A later revision",
    });
    const replacementResponse = await context.request.post(
      `/api/v1/publications/${original.id}/replace`,
      {
        data: {
          post_version: revised.version,
          publication_post_version: original.post_version,
          scheduled_at: original.scheduled_at,
        },
        headers: {
          "X-Beweft-Request": "1",
          Origin: process.env.BEWEFT_TEST_URL!,
        },
      },
    );
    expect(replacementResponse.ok(), await replacementResponse.text()).toBe(
      true,
    );
    const replacement = (await replacementResponse.json()).data.publication;
    await expect
      .poll(() => Date.now(), { timeout: 40000, intervals: [250, 500, 1000] })
      .toBeGreaterThan(Date.parse(saved.scheduled_at) + 6000);
    await page.reload();
    await page.getByRole("button", { name: "Review pending schedule" }).click();
    await page.getByRole("button", { name: "Retry original schedule" }).click();
    await expect(page.getByText(/1 item is now canceled/)).toBeVisible();
    expect(requests[1]).toEqual(requests[0]);
    const receipt = await read(context, `/schedule-requests/${saved.id}`);
    expect(receipt.publications.map((row: any) => row.id)).toEqual([
      original.id,
    ]);
    expect(receipt.publications[0].state).toBe("canceled");
    const rows = await read(context, `/publications?brand_id=${f.brand.id}`);
    expect(rows.map((row: any) => row.id).sort()).toEqual(
      [original.id, replacement.id].sort(),
    );
    expect((await read(context, `/posts/${f.post.id}`)).version).toBe(
      revised.version,
    );
    expect(f.patches()).toBe(1);
  });
});

test("real metadata edit before channel admission rejects and requires a reviewed new schedule", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write, true);
    const release = await holdLock(
      `SELECT pg_advisory_xact_lock(hashtextextended('${f.channel.id}',0))`,
    );
    const requests: any[] = [];
    page.on("request", (request) => {
      if (
        request.method() === "POST" &&
        routeFor(f.post.id).test(request.url())
      )
        requests.push(request.postDataJSON());
    });
    try {
      await start(page);
      await waitForLock("pg_advisory_xact_lock");
      const first = await pending(page, f.key);
      expect(first.media_versions).toEqual({ [f.media.id]: 1 });
      expect(
        (
          await context.request.get(`/api/v1/schedule-requests/${first.id}`)
        ).status(),
      ).toBe(404);
      await page.getByRole("button", { name: "Close", exact: true }).click();
      await page
        .getByRole("button", { name: "Review pending schedule" })
        .click();
      await page
        .getByRole("button", { name: "Retry original schedule" })
        .click();
      await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
        "still in progress",
      );
      expect((await pending(page, f.key)).uncertain).toBe(true);
      const changed = await patch(context, `/media/${f.media.id}`, {
        metadata_version: 1,
        alt_text: "The newer description",
      });
      expect(changed.metadata_version).toBe(2);
      await release();
      await expect(
        page.getByText(/Media descriptions changed\. Refresh them/),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Refresh descriptions" }),
      ).toBeEnabled();
      expect((await pending(page, f.key)).uncertain).toBe(false);
      expect(
        await read(context, `/publications?brand_id=${f.brand.id}`),
      ).toEqual([]);
      await page.getByRole("button", { name: "Refresh descriptions" }).click();
      await expect(
        page.getByText("receipt.png: The newer description"),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Schedule post" }),
      ).toBeEnabled();
      expect(await pending(page, f.key)).toBeNull();
      await page.getByRole("button", { name: "Schedule post" }).click();
      await expect
        .poll(
          async () =>
            (await read(context, `/publications?brand_id=${f.brand.id}`))
              .length,
        )
        .toBe(1);
      expect(requests).toHaveLength(3);
      expect(requests[1]).toEqual(requests[0]);
      expect(requests[2].request_id).not.toBe(first.id);
      expect(requests[2].media_versions).toEqual({ [f.media.id]: 2 });
      expect(
        (await read(context, `/publications?brand_id=${f.brand.id}`))[0].payload
          .media_metadata,
      ).toEqual([{ id: f.media.id, alt_text: "The newer description" }]);
    } finally {
      await release();
    }
  });
});

test("real media edit after snapshot locks preserves the original through lost response and reload", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write, true);
    const before = await patch(context, `/media/${f.media.id}`, {
      metadata_version: 1,
      alt_text: "Approved description",
    });
    const name = `fixture_gate_${f.post.id.replaceAll("-", "")}`;
    const gate = `fixture-publication-${f.post.id}`;
    sql(
      `CREATE FUNCTION ${name}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.post_id='${f.post.id}' THEN PERFORM pg_advisory_xact_lock(hashtextextended('${gate}',0)); END IF; RETURN NEW; END $$; CREATE TRIGGER ${name} BEFORE INSERT ON publications FOR EACH ROW EXECUTE FUNCTION ${name}();`,
    );
    const release = await holdLock(
      `SELECT pg_advisory_xact_lock(hashtextextended('${gate}',0))`,
    );
    const requests: any[] = [];
    const finished = deferred();
    await page.route(routeFor(f.post.id), async (route) => {
      requests.push(route.request().postDataJSON());
      if (requests.length !== 1) return route.continue();
      try {
        await route.fetch();
      } finally {
        finished.resolve();
      }
      await route.abort("failed").catch(() => {});
    });
    try {
      await start(page);
      await waitForLock("INSERT INTO publications");
      const saved = await pending(page, f.key);
      expect(saved.media_versions).toEqual({
        [f.media.id]: before.metadata_version,
      });
      expect(
        (
          await context.request.get(`/api/v1/schedule-requests/${saved.id}`)
        ).status(),
      ).toBe(404);
      const editing = patch(context, `/media/${f.media.id}`, {
        metadata_version: before.metadata_version,
        alt_text: "After the snapshot",
      });
      await waitForLock("UPDATE media SET alt_text");
      await page.getByRole("button", { name: "Close", exact: true }).click();
      await page.reload();
      await release();
      expect((await editing).metadata_version).toBe(
        before.metadata_version + 1,
      );
      await finished.promise;
      await expect
        .poll(async () =>
          (
            await context.request.get(`/api/v1/schedule-requests/${saved.id}`)
          ).status(),
        )
        .toBe(200);
      await page.reload();
      await page
        .getByRole("button", { name: "Review pending schedule" })
        .click();
      await expect(
        page.getByText(/preview has changed or is unavailable/),
      ).toBeVisible();
      await page
        .getByRole("button", { name: "Retry original schedule" })
        .click();
      await expect(
        page.getByText(/The original request is confirmed/),
      ).toBeVisible();
      expect(requests[1]).toEqual(requests[0]);
      const receipt = await read(context, `/schedule-requests/${saved.id}`);
      expect(receipt.request.media_versions).toEqual({
        [f.media.id]: before.metadata_version,
      });
      const rows = await read(context, `/publications?brand_id=${f.brand.id}`);
      expect(rows).toHaveLength(1);
      expect(rows[0].id).toBe(receipt.publications[0].id);
      expect(rows[0].payload.media_metadata).toEqual([
        { id: f.media.id, alt_text: "Approved description" },
      ]);
      expect((await read(context, `/posts/${f.post.id}`)).version).toBe(
        saved.post.version,
      );
      expect(f.patches()).toBe(1);
    } finally {
      await release();
      sql(
        `DROP TRIGGER IF EXISTS ${name} ON publications; DROP FUNCTION IF EXISTS ${name}();`,
      );
    }
  });
});

test("real schedule maps all selected effective variants including an explicit empty override", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Real variant media map" });
    const channels = [];
    for (const name of ["Shared", "Empty", "Custom"])
      channels.push(
        await write("/channels", {
          brand_id: brand.id,
          provider: "manual",
          name,
          timezone: "UTC",
        }),
      );
    const a = await upload(context, brand.id, "shared.png");
    const b = await upload(context, brand.id, "custom.png");
    const post = await write("/posts", {
      brand_id: brand.id,
      title: "Variant map draft",
      text: "Fixture",
      media_ids: [a.id],
      variants: [
        { channel_id: channels[0].id },
        { channel_id: channels[1].id, media_ids: [] },
        { channel_id: channels[2].id, media_ids: [b.id] },
      ],
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    let sent: any;
    page.on("request", (request) => {
      if (request.method() === "POST" && routeFor(post.id).test(request.url()))
        sent = request.postDataJSON();
    });
    await start(page);
    await expect
      .poll(
        async () =>
          (await read(context, `/publications?brand_id=${brand.id}`)).length,
      )
      .toBe(3);
    expect(sent.media_versions).toEqual({ [a.id]: 1, [b.id]: 1 });
    expect(sent.channel_ids).toEqual(
      channels.map((channel) => channel.id).sort(),
    );
    const receipt = await read(
      context,
      `/schedule-requests/${sent.request_id}`,
    );
    expect(receipt.request.media_versions).toEqual(sent.media_versions);
    const rows = await read(context, `/publications?brand_id=${brand.id}`);
    const byChannel = Object.fromEntries(
      rows.map((row: any) => [row.channel_id, row.payload.media_metadata]),
    );
    expect(byChannel[channels[0].id]).toEqual([{ id: a.id, alt_text: "" }]);
    expect(byChannel[channels[1].id]).toEqual([]);
    expect(byChannel[channels[2].id]).toEqual([{ id: b.id, alt_text: "" }]);
  });
});

test("real queue recovery preserves its null time after the original is rescheduled", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    await write("/schedules", {
      channel_id: f.channel.id,
      days: [1, 2, 3, 4, 5, 6, 7],
      time: "23:58",
      timezone: "UTC",
    });
    const committed = deferred();
    const requests: any[] = [];
    await page.route(routeFor(f.post.id), async (route) => {
      requests.push(route.request().postDataJSON());
      if (requests.length !== 1) return route.continue();
      const response = await route.fetch();
      expect(response.ok(), await response.text()).toBe(true);
      committed.resolve();
      await route.abort("failed").catch(() => {});
    });
    await page.goto("/#drafts");
    await page.getByRole("button", { name: "Edit draft", exact: true }).click();
    await page.getByLabel("When should this go out?").selectOption("queue");
    await page.getByRole("button", { name: "Add to queue" }).click();
    await committed.promise;
    await expect(
      page.getByRole("button", { name: "Retry original schedule" }),
    ).toBeEnabled();
    const saved = await pending(page, f.key);
    expect(saved.scheduled_at).toBeNull();
    expect(saved.media_versions).toEqual({});
    const receipt = await read(context, `/schedule-requests/${saved.id}`);
    expect(receipt.request.scheduled_at).toBeNull();
    expect(receipt.request.media_versions).toEqual({});
    const later = new Date(Date.now() + 2 * 86400_000).toISOString();
    const changed = await context.request.post(
      `/api/v1/publications/${receipt.publications[0].id}/reschedule`,
      {
        data: { scheduled_at: later },
        headers: {
          "X-Beweft-Request": "1",
          Origin: process.env.BEWEFT_TEST_URL!,
        },
      },
    );
    expect(changed.ok(), await changed.text()).toBe(true);
    await page.reload();
    await page.getByRole("button", { name: "Review pending schedule" }).click();
    await page.getByRole("button", { name: "Retry original schedule" }).click();
    await expect(
      page.getByText(/The original request is confirmed/),
    ).toBeVisible();
    expect(requests[1]).toEqual(requests[0]);
    const rows = await read(context, `/publications?brand_id=${f.brand.id}`);
    expect(rows).toHaveLength(1);
    expect(rows[0].id).toBe(receipt.publications[0].id);
    expect(rows[0].scheduling_mode).toBe("exact");
    expect(Date.parse(rows[0].scheduled_at)).toBe(Date.parse(later));
    expect((await read(context, `/posts/${f.post.id}`)).version).toBe(
      saved.post.version,
    );
    expect(f.patches()).toBe(1);
  });
});

test("a failed receipt read after a real successful POST keeps the outcome unresolved", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    let receiptReads = 0;
    await page.route("**/api/v1/schedule-requests/*", (route) => {
      receiptReads++;
      return receiptReads === 1
        ? route.fulfill({
            status: 403,
            json: {
              error: {
                code: "unavailable",
                message: "Receipt temporarily unavailable",
              },
            },
          })
        : route.continue();
    });
    await start(page);
    await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
      "Receipt temporarily unavailable",
    );
    const saved = await pending(page, f.key);
    expect(saved.uncertain).toBe(true);
    await expect(
      page.getByRole("button", { name: "Release and refresh" }),
    ).toHaveCount(0);
    expect(
      await read(context, `/publications?brand_id=${f.brand.id}`),
    ).toHaveLength(1);
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(
      page.getByText(/The original request is confirmed/),
    ).toBeVisible();
    expect(await pending(page, f.key)).toBeNull();
    expect((await read(context, `/posts/${f.post.id}`)).version).toBe(
      saved.post.version,
    );
    expect(f.patches()).toBe(1);
  });
});

test("a media-conflict error from receipt lookup cannot release a committed schedule", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    let receiptReads = 0;
    await page.route("**/api/v1/schedule-requests/*", (route) => {
      receiptReads++;
      return receiptReads === 1
        ? route.fulfill({
            status: 409,
            json: {
              error: {
                code: "media_version_conflict",
                message: "Receipt check failed",
              },
            },
          })
        : route.continue();
    });
    await start(page);
    await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
      "Receipt check failed",
    );
    const saved = await pending(page, f.key);
    expect(saved.uncertain).toBe(true);
    await expect(
      page.getByRole("button", { name: "Refresh descriptions" }),
    ).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: "Release and refresh" }),
    ).toHaveCount(0);
    expect(
      await read(context, `/publications?brand_id=${f.brand.id}`),
    ).toHaveLength(1);
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(
      page.getByText(/The original request is confirmed/),
    ).toBeVisible();
    expect(await pending(page, f.key)).toBeNull();
    expect(f.patches()).toBe(1);
  });
});

test("a generic 400 and 404 cannot release a transaction held before commit", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    const release = await holdLock(
      `SELECT pg_advisory_xact_lock(hashtextextended('${f.channel.id}',0))`,
    );
    const forwarded = deferred();
    let upstream: ReturnType<typeof context.request.post> | undefined;
    await page.route(routeFor(f.post.id), async (route) => {
      upstream = context.request.post(
        `/api/v1/posts/${f.post.id}/schedule?view=summary`,
        {
          data: route.request().postDataJSON(),
          headers: {
            "X-Beweft-Request": "1",
            Origin: process.env.BEWEFT_TEST_URL!,
          },
        },
      );
      await waitForLock("pg_advisory_xact_lock");
      forwarded.resolve();
      await route.fulfill({
        status: 400,
        json: {
          error: { code: "invalid_request", message: "Proxy rejected request" },
        },
      });
    });
    try {
      await start(page);
      await forwarded.promise;
      await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
        "Proxy rejected request",
      );
      const original = await pending(page, f.key);
      expect(original.uncertain).toBe(true);
      expect(
        (
          await context.request.get(`/api/v1/schedule-requests/${original.id}`)
        ).status(),
      ).toBe(404);
      await page
        .getByRole("button", { name: "Check scheduling outcome" })
        .click();
      await expect(
        page.getByText(/No request record is visible yet/),
      ).toBeVisible();
      await expect(
        page.getByLabel("Post text", { exact: true }),
      ).toBeDisabled();
      await page
        .getByRole("button", { name: "Stop original schedule" })
        .click();
      await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
        "still in progress",
      );
      expect((await pending(page, f.key)).id).toBe(original.id);
      expect(f.patches()).toBe(1);
      await release();
      const response = await upstream!;
      expect(response.ok(), await response.text()).toBe(true);
      await page
        .getByRole("button", { name: "Check scheduling outcome" })
        .click();
      await expect(
        page.getByText(/original request is confirmed/),
      ).toBeVisible();
      expect(
        await read(context, `/publications?brand_id=${f.brand.id}`),
      ).toHaveLength(1);
      expect((await read(context, `/posts/${f.post.id}`)).version).toBe(
        original.post.version,
      );
      expect(f.patches()).toBe(1);
    } finally {
      await release();
      await upstream?.catch(() => {});
    }
  });
});

test("retirement before a delayed original POST prevents it from scheduling", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    let body: any;
    await page.route(routeFor(f.post.id), async (route) => {
      body = route.request().postDataJSON();
      await route.fulfill({
        status: 400,
        json: { error: { code: "invalid_request", message: "Response lost" } },
      });
    });
    await start(page);
    await expect(
      page.getByRole("button", { name: "Stop original schedule" }),
    ).toBeEnabled();
    const original = await pending(page, f.key);
    await page.getByRole("button", { name: "Stop original schedule" }).click();
    await expect(page.getByText(/original schedule was stopped/)).toBeVisible();
    expect(await pending(page, f.key)).toBeNull();
    const receipt = await read(context, `/schedule-requests/${original.id}`);
    expect(receipt.outcome).toBe("rejected");
    expect(receipt.error.code).toBe("schedule_request_retired");
    const delayed = await context.request.post(
      `/api/v1/posts/${f.post.id}/schedule?view=summary`,
      {
        data: body,
        headers: {
          "X-Beweft-Request": "1",
          Origin: process.env.BEWEFT_TEST_URL!,
        },
      },
    );
    expect(delayed.status()).toBe(409);
    expect((await delayed.json()).error.code).toBe("schedule_request_retired");
    expect(await read(context, `/publications?brand_id=${f.brand.id}`)).toEqual(
      [],
    );
    expect((await read(context, `/posts/${f.post.id}`)).version).toBe(
      original.post.version,
    );
    expect(f.patches()).toBe(1);
  });
});

test("stopping a committed request returns its original accepted publication", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    let originalRows: any[] = [];
    await page.route(routeFor(f.post.id), async (route) => {
      const response = await route.fetch();
      expect(response.ok(), await response.text()).toBe(true);
      originalRows = (await response.json()).data;
      await route.abort("failed");
    });
    await start(page);
    await expect(
      page.getByRole("button", { name: "Stop original schedule" }),
    ).toBeEnabled();
    const saved = await pending(page, f.key);
    await page.getByRole("button", { name: "Stop original schedule" }).click();
    await expect(page.getByText(/original request is confirmed/)).toBeVisible();
    expect(await pending(page, f.key)).toBeNull();
    const rows = await read(
      context,
      `/publications?brand_id=${f.brand.id}&view=summary`,
    );
    expect(rows.map((row: any) => row.id)).toEqual(
      originalRows.map((row: any) => row.id),
    );
    expect((await read(context, `/posts/${f.post.id}`)).version).toBe(
      saved.post.version,
    );
    expect(f.patches()).toBe(1);
  });
});

test("a lost retirement response recovers its durable rejection after reload", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    let body: any;
    await page.route(routeFor(f.post.id), async (route) => {
      body = route.request().postDataJSON();
      await route.abort("failed");
    });
    await page.route("**/api/v1/schedule-requests/*/retire", async (route) => {
      const response = await route.fetch();
      expect(response.ok(), await response.text()).toBe(true);
      await route.abort("failed");
    });
    await start(page);
    await expect(
      page.getByRole("button", { name: "Stop original schedule" }),
    ).toBeEnabled();
    const saved = await pending(page, f.key);
    await page.getByRole("button", { name: "Stop original schedule" }).click();
    await expect(page.getByRole("dialog").getByRole("alert")).toBeVisible();
    expect((await pending(page, f.key)).id).toBe(saved.id);
    expect(
      (await read(context, `/schedule-requests/${saved.id}`)).outcome,
    ).toBe("rejected");
    await page.reload();
    await page.getByRole("button", { name: "Review pending schedule" }).click();
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(page.getByText(/original schedule was stopped/)).toBeVisible();
    expect(await pending(page, f.key)).toBeNull();
    const delayed = await context.request.post(
      `/api/v1/posts/${f.post.id}/schedule?view=summary`,
      {
        data: body,
        headers: {
          "X-Beweft-Request": "1",
          Origin: process.env.BEWEFT_TEST_URL!,
        },
      },
    );
    expect(delayed.status()).toBe(409);
    expect((await delayed.json()).error.code).toBe("schedule_request_retired");
    expect(await read(context, `/publications?brand_id=${f.brand.id}`)).toEqual(
      [],
    );
  });
});

test("a rejected receipt survives a deleted draft and can be recovered as new content", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    await page.route(routeFor(f.post.id), (route) => route.abort("failed"));
    await page.route("**/api/v1/schedule-requests/*/retire", async (route) => {
      const response = await route.fetch();
      expect(response.ok(), await response.text()).toBe(true);
      await route.abort("failed");
    });
    await start(page);
    await expect(
      page.getByRole("button", { name: "Stop original schedule" }),
    ).toBeEnabled();
    const saved = await pending(page, f.key);
    await page.getByRole("button", { name: "Stop original schedule" }).click();
    await expect(page.getByRole("dialog").getByRole("alert")).toBeVisible();
    expect(
      (await read(context, `/schedule-requests/${saved.id}`)).outcome,
    ).toBe("rejected");
    const deleted = await context.request.delete(`/api/v1/posts/${f.post.id}`, {
      headers: {
        "X-Beweft-Request": "1",
        Origin: process.env.BEWEFT_TEST_URL!,
      },
    });
    expect(deleted.ok(), await deleted.text()).toBe(true);
    await page.reload();
    await page.getByRole("button", { name: "Review pending schedule" }).click();
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(page.getByText(/draft was deleted/)).toBeVisible();
    expect(await pending(page, f.key)).toBeNull();
    await expect(page.getByLabel("Post text", { exact: true })).toHaveValue(
      "Approved draft",
    );
    await page.getByLabel("When should this go out?").selectOption("draft");
    await page.getByRole("button", { name: "Save draft" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    const posts = await read(context, `/posts?brand_id=${f.brand.id}`);
    expect(posts).toHaveLength(1);
    expect(posts[0].id).not.toBe(f.post.id);
    expect(posts[0].text).toBe("Approved draft");
    expect(await read(context, `/publications?brand_id=${f.brand.id}`)).toEqual(
      [],
    );
  });
});

test("retirement of an unverifiable deleted draft leaves the outcome unresolved", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    await page.route(routeFor(f.post.id), (route) => route.abort("failed"));
    await start(page);
    await expect(
      page.getByRole("button", { name: "Stop original schedule" }),
    ).toBeEnabled();
    const saved = await pending(page, f.key);
    const deleted = await context.request.delete(`/api/v1/posts/${f.post.id}`, {
      headers: {
        "X-Beweft-Request": "1",
        Origin: process.env.BEWEFT_TEST_URL!,
      },
    });
    expect(deleted.ok(), await deleted.text()).toBe(true);
    const stopped = page.waitForResponse((response) =>
      response.url().endsWith(`/schedule-requests/${saved.id}/retire`),
    );
    await page.getByRole("button", { name: "Stop original schedule" }).click();
    expect((await stopped).status()).toBe(404);
    await expect(page.getByRole("dialog").getByRole("alert")).toBeVisible();
    expect((await pending(page, f.key)).id).toBe(saved.id);
    await expect(page.getByLabel("Post text", { exact: true })).toBeDisabled();
    await page.reload();
    await page.getByRole("button", { name: "Review pending schedule" }).click();
    await expect(page.getByLabel("Post text", { exact: true })).toBeDisabled();
    expect((await pending(page, f.key)).id).toBe(saved.id);
  });
});

test("retirement cannot replace a different schedule already bound to the same ID", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    let body: any;
    await page.route(routeFor(f.post.id), (route) => {
      body = route.request().postDataJSON();
      return route.abort("failed");
    });
    await start(page);
    await expect(
      page.getByRole("button", { name: "Stop original schedule" }),
    ).toBeEnabled();
    const saved = await pending(page, f.key);
    const other = await write("/posts", {
      brand_id: f.brand.id,
      title: "Other draft",
      text: "Different schedule",
      media_ids: [],
      variants: [{ channel_id: f.channel.id }],
    });
    const otherResponse = await context.request.post(
      `/api/v1/posts/${other.id}/schedule?view=summary`,
      {
        data: { ...body, version: other.version },
        headers: {
          "X-Beweft-Request": "1",
          Origin: process.env.BEWEFT_TEST_URL!,
        },
      },
    );
    expect(otherResponse.ok(), await otherResponse.text()).toBe(true);
    const otherRows = (await otherResponse.json()).data;
    await page.getByRole("button", { name: "Stop original schedule" }).click();
    await expect(
      page.getByRole("status").getByText(/server record does not match/),
    ).toBeVisible();
    expect((await pending(page, f.key)).rejection_code).toBe(
      "schedule_request_conflict",
    );
    await page.reload();
    await page.getByRole("button", { name: "Review pending schedule" }).click();
    await expect(
      page.getByRole("status").getByText(/server record does not match/),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Retry original schedule" }),
    ).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: "Stop original schedule" }),
    ).toHaveCount(0);
    const rows = await read(
      context,
      `/publications?brand_id=${f.brand.id}&view=summary`,
    );
    expect(rows.map((row: any) => row.id)).toEqual(
      otherRows.map((row: any) => row.id),
    );
    expect((await read(context, `/posts/${f.post.id}`)).version).toBe(
      saved.post.version,
    );
  });
});

async function twoChannelSetup(
  page: Page,
  context: BrowserContext,
  write: (path: string, body: unknown) => Promise<any>,
) {
  const f = await setup(page, context, write);
  const second = await write("/channels", {
    brand_id: f.brand.id,
    provider: "manual",
    name: "Second recovery channel",
    timezone: "UTC",
  });
  await patch(context, `/posts/${f.post.id}`, {
    version: f.post.version,
    variants: [{ channel_id: f.channel.id }, { channel_id: second.id }],
  });
  return { ...f, second };
}

test("a partial successful POST preserves acceptance before a contradictory rejected receipt", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await twoChannelSetup(page, context, write);
    let originals: any[] = [];
    await page.route(routeFor(f.post.id), async (route) => {
      const response = await route.fetch();
      expect(response.ok(), await response.text()).toBe(true);
      originals = (await response.json()).data;
      expect(originals).toHaveLength(2);
      await route.fulfill({ json: { data: [originals[0]] } });
    });
    await page.route("**/api/v1/schedule-requests/*", async (route) => {
      const response = await route.fetch();
      expect(response.ok(), await response.text()).toBe(true);
      const data = (await response.json()).data;
      data.outcome = "rejected";
      data.error = { code: "schedule_request_retired", message: "Stopped" };
      data.publications = [];
      await route.fulfill({ json: { data } });
    });
    await start(page);
    await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
      "incomplete",
    );
    const before = await pending(page, f.key);
    expect(before.observed_publications).toEqual([
      { channel_id: originals[0].channel_id, publication_id: originals[0].id },
    ]);
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(
      page.getByRole("status").getByText(/server record does not match/),
    ).toBeVisible();
    const saved = await pending(page, f.key);
    expect(saved.observed_publications).toEqual(before.observed_publications);
    expect(saved.rejection_code).toBe("schedule_request_conflict");
    await page.reload();
    await page.getByRole("button", { name: "Review pending schedule" }).click();
    await expect(page.getByLabel("Post text", { exact: true })).toBeDisabled();
    await expect(
      page.getByRole("button", { name: "Stop original schedule" }),
    ).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: "Schedule post" }),
    ).toHaveCount(0);
    const actual = await read(
      context,
      `/publications?brand_id=${f.brand.id}&view=summary`,
    );
    expect(actual.map((row: any) => row.id).sort()).toEqual(
      originals.map((row: any) => row.id).sort(),
    );
    expect((await read(context, `/posts/${f.post.id}`)).version).toBe(
      saved.post.version,
    );
    expect(f.patches()).toBe(1);
  });
});

test("a receipt cannot swap observed publication IDs between channels", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await twoChannelSetup(page, context, write);
    let originals: any[] = [];
    await page.route(routeFor(f.post.id), async (route) => {
      const response = await route.fetch();
      expect(response.ok(), await response.text()).toBe(true);
      originals = (await response.json()).data;
      await route.fulfill({ response });
    });
    await page.route("**/api/v1/schedule-requests/*", async (route) => {
      const response = await route.fetch();
      expect(response.ok(), await response.text()).toBe(true);
      const data = (await response.json()).data;
      const [first, second] = data.publications;
      [first.channel_id, second.channel_id] = [
        second.channel_id,
        first.channel_id,
      ];
      await route.fulfill({ json: { data } });
    });
    await start(page);
    await expect(
      page.getByRole("status").getByText(/server record does not match/),
    ).toBeVisible();
    const saved = await pending(page, f.key);
    expect(saved.rejection_code).toBe("schedule_request_conflict");
    expect(saved.observed_publication_ids.sort()).toEqual(
      originals.map((row) => row.id).sort(),
    );
    expect(saved.observed_publications).toEqual(
      expect.arrayContaining(
        originals.map((row) => ({
          channel_id: row.channel_id,
          publication_id: row.id,
        })),
      ),
    );
    expect(saved.observed_publications).toHaveLength(4);
    await page.reload();
    await page.getByRole("button", { name: "Review pending schedule" }).click();
    await expect(
      page.getByRole("button", { name: "Stop original schedule" }),
    ).toHaveCount(0);
    expect(
      await read(context, `/publications?brand_id=${f.brand.id}&view=summary`),
    ).toHaveLength(2);
  });
});

test("partial GET receipts accumulate bindings and cannot later become a rejection", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await twoChannelSetup(page, context, write);
    let originals: any[] = [],
      reads = 0;
    await page.route(routeFor(f.post.id), async (route) => {
      const response = await route.fetch();
      expect(response.ok(), await response.text()).toBe(true);
      originals = (await response.json()).data;
      await route.abort("failed");
    });
    await page.route("**/api/v1/schedule-requests/*", async (route) => {
      const response = await route.fetch();
      expect(response.ok(), await response.text()).toBe(true);
      const data = (await response.json()).data;
      reads++;
      if (reads <= 2) data.publications = [data.publications[reads - 1]];
      else {
        data.outcome = "rejected";
        data.error = { code: "schedule_request_retired", message: "Stopped" };
        data.publications = [];
      }
      await route.fulfill({ json: { data } });
    });
    await start(page);
    await expect(
      page.getByRole("button", { name: "Check scheduling outcome" }),
    ).toBeEnabled();
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(page.getByText(/only part of this request/)).toBeVisible();
    const first = await pending(page, f.key);
    expect(first.observed_publications).toHaveLength(1);
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect
      .poll(
        async () => (await pending(page, f.key)).observed_publications?.length,
      )
      .toBe(2);
    const second = await pending(page, f.key);
    expect(second.observed_publications).toEqual(
      expect.arrayContaining(first.observed_publications),
    );
    expect(second.observed_publications).toEqual(
      expect.arrayContaining(
        originals.map((row) => ({
          channel_id: row.channel_id,
          publication_id: row.id,
        })),
      ),
    );
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(
      page.getByRole("status").getByText(/server record does not match/),
    ).toBeVisible();
    expect((await pending(page, f.key)).observed_publications).toEqual(
      second.observed_publications,
    );
    expect(
      await read(context, `/publications?brand_id=${f.brand.id}&view=summary`),
    ).toHaveLength(2);
  });
});

test("an exact saved timestamp retains seconds, microseconds and offset after retirement", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write, true);
    const base = new Date(Date.now() + 7 * 86400_000);
    base.setUTCSeconds(37, 0);
    const shifted = new Date(base.getTime() + 330 * 60000)
      .toISOString()
      .slice(0, 19);
    const exact = `${shifted}.123456+05:30`;
    const expectedUtc = `${base.toISOString().slice(0, 19)}.123456Z`;
    const requests: any[] = [];
    const firstDone = deferred();
    await page.route(routeFor(f.post.id), async (route) => {
      requests.push(route.request().postDataJSON());
      if (requests.length !== 1) return route.continue();
      await page.evaluate(
        ({ key, exact }) => {
          const saved = JSON.parse(localStorage.getItem(key)!);
          saved.scheduled_at = exact;
          localStorage.setItem(key, JSON.stringify(saved));
        },
        { key: f.key, exact },
      );
      const changed = await patch(context, `/media/${f.media!.id}`, {
        metadata_version: 1,
        alt_text: "Changed before the original scheduling attempt",
      });
      expect(changed.metadata_version).toBe(2);
      const response = await route.fetch({
        postData: JSON.stringify({ ...requests[0], scheduled_at: exact }),
      });
      expect(response.status()).toBe(409);
      expect((await response.json()).error.code).toBe("media_version_conflict");
      await route.fulfill({ response });
      firstDone.resolve();
    });
    const localBase = new Date(
      base.getTime() - base.getTimezoneOffset() * 60000,
    )
      .toISOString()
      .slice(0, 19);
    await start(page, localBase);
    await expect.poll(() => requests.length).toBe(1);
    expect(requests[0].scheduled_at).toMatch(/:37\.000Z$/);
    await firstDone.promise;
    await expect
      .poll(async () => (await pending(page, f.key))?.scheduled_at)
      .toBe(exact);
    await page.reload();
    await page.getByRole("button", { name: "Review pending schedule" }).click();
    await expect(page.getByText(`Exact saved time: ${exact}`)).toBeVisible();
    const shown = await page.getByLabel(/Date and time/).inputValue();
    expect(shown).toMatch(/:37\.123$/);
    await page.getByRole("button", { name: "Stop original schedule" }).click();
    await expect(page.getByText(/original schedule was stopped/)).toBeVisible();
    await expect(page.getByText(`Exact saved time: ${exact}`)).toBeVisible();
    await page.getByRole("button", { name: "Cancel", exact: true }).click();
    await page.reload();
    await page.getByRole("button", { name: "Edit draft", exact: true }).click();
    await expect(page.getByLabel("When should this go out?")).toHaveValue(
      "exact",
    );
    await expect(page.getByText(`Exact saved time: ${exact}`)).toBeVisible();
    await page
      .getByLabel("Working title")
      .fill("Corrected without changing the time");
    await page.getByRole("button", { name: "Schedule post" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    expect(requests).toHaveLength(2);
    expect(requests[1].scheduled_at).toBe(exact);
    expect(requests[1].request_id).not.toBe(requests[0].request_id);
    expect(requests[1].version).toBe(requests[0].version + 1);
    expect(requests[1].media_versions).toEqual({ [f.media!.id]: 2 });
    const receipt = await read(
      context,
      `/schedule-requests/${requests[1].request_id}`,
    );
    expect(receipt.request.scheduled_at).toBe(expectedUtc);
    expect(
      await read(context, `/publications?brand_id=${f.brand.id}&view=summary`),
    ).toHaveLength(1);
  });
});

test("a retired queue choice survives reload and a deliberate mode and time edit replaces it", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    const key = `beweft.schedule-correction:${f.owner.id}:${f.brand.id}:${f.post.id}`;
    const requests: any[] = [];
    await page.route(routeFor(f.post.id), (route) => {
      requests.push(route.request().postDataJSON());
      return requests.length === 1 ? route.abort("failed") : route.continue();
    });
    await page.goto("/#drafts");
    await page.getByRole("button", { name: "Edit draft", exact: true }).click();
    await page.getByLabel("When should this go out?").selectOption("queue");
    await page.getByRole("button", { name: "Add to queue" }).click();
    await page.getByRole("button", { name: "Stop original schedule" }).click();
    await expect(page.getByText(/original schedule was stopped/)).toBeVisible();
    expect(
      JSON.parse((await page.evaluate((k) => localStorage.getItem(k), key))!),
    ).toMatchObject({ mode: "queue", scheduled_at: null });
    await page.getByRole("button", { name: "Cancel", exact: true }).click();
    await page.reload();
    await page.getByRole("button", { name: "Edit draft", exact: true }).click();
    await expect(page.getByLabel("When should this go out?")).toHaveValue(
      "queue",
    );
    await page.getByLabel("When should this go out?").selectOption("exact");
    const chosen = localTime(4 * 86400_000);
    await page.getByLabel(/Date and time/).fill(chosen);
    expect(await page.evaluate((k) => localStorage.getItem(k), key)).toBeNull();
    await page.getByRole("button", { name: "Schedule post" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    expect(requests[1].request_id).not.toBe(requests[0].request_id);
    expect(requests[1].scheduled_at).toBe(new Date(chosen).toISOString());
    expect(
      await read(context, `/publications?brand_id=${f.brand.id}&view=summary`),
    ).toHaveLength(1);
  });
});

test("deliberately editing a restored exact time uses the new time", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    const key = `beweft.schedule-correction:${f.owner.id}:${f.brand.id}:${f.post.id}`;
    const exact = new Date(Date.now() + 8 * 86400_000).toISOString();
    await page.goto("/#drafts");
    await page.evaluate(
      ({ key, f, exact }) => {
        localStorage.setItem(
          key,
          JSON.stringify({
            revision: crypto.randomUUID(),
            owner_id: f.owner.id,
            brand_id: f.brand.id,
            post_id: f.post.id,
            mode: "exact",
            scheduled_at: exact,
          }),
        );
      },
      { key, f: { owner: f.owner, brand: f.brand, post: f.post }, exact },
    );
    let request: any;
    await page.route(routeFor(f.post.id), (route) => {
      request = route.request().postDataJSON();
      return route.continue();
    });
    await page.getByRole("button", { name: "Edit draft", exact: true }).click();
    await expect(page.getByText(`Exact saved time: ${exact}`)).toBeVisible();
    const changed = localTime(3 * 86400_000);
    await page.getByLabel(/Date and time/).fill(changed);
    expect(
      await page.evaluate((key) => localStorage.getItem(key), key),
    ).toBeNull();
    await expect(page.getByText(`Exact saved time: ${exact}`)).toHaveCount(0);
    await page.getByRole("button", { name: "Schedule post" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    expect(request.scheduled_at).toBe(new Date(changed).toISOString());
  });
});

test("a correction storage failure retains the rejected request until the time is saved", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    const key = `beweft.schedule-correction:${f.owner.id}:${f.brand.id}:${f.post.id}`;
    await page.route(routeFor(f.post.id), (route) => route.abort("failed"));
    await start(page);
    await expect(
      page.getByRole("button", { name: "Stop original schedule" }),
    ).toBeEnabled();
    const before = await pending(page, f.key);
    await page.evaluate((key) => {
      const set = Storage.prototype.setItem;
      (window as any).restoreCorrectionStorage = () => {
        Storage.prototype.setItem = set;
      };
      Storage.prototype.setItem = function (item, value) {
        if (item === key) throw new Error("Correction storage unavailable");
        return set.call(this, item, value);
      };
    }, key);
    await page.getByRole("button", { name: "Stop original schedule" }).click();
    await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
      "Correction storage unavailable",
    );
    expect((await pending(page, f.key)).id).toBe(before.id);
    expect(await page.evaluate((k) => localStorage.getItem(k), key)).toBeNull();
    await page.evaluate(() => (window as any).restoreCorrectionStorage());
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(page.getByText(/original schedule was stopped/)).toBeVisible();
    expect(await pending(page, f.key)).toBeNull();
    expect(
      JSON.parse((await page.evaluate((k) => localStorage.getItem(k), key))!)
        .scheduled_at,
    ).toBe(before.scheduled_at);
  });
});

test("a damaged preference during retirement requires explicit review before recovery", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    const key = `beweft.schedule-correction:${f.owner.id}:${f.brand.id}:${f.post.id}`;
    await page.route(routeFor(f.post.id), (route) => route.abort("failed"));
    await start(page);
    await expect(
      page.getByRole("button", { name: "Stop original schedule" }),
    ).toBeEnabled();
    const saved = await pending(page, f.key);
    await page.evaluate((key) => localStorage.setItem(key, "{damaged"), key);
    await page.getByRole("button", { name: "Stop original schedule" }).click();
    await expect(
      page.getByText("Other saved choice: unreadable."),
    ).toBeVisible();
    expect(await page.evaluate((key) => localStorage.getItem(key), key)).toBe(
      "{damaged",
    );
    expect((await pending(page, f.key)).id).toBe(saved.id);
    await page
      .getByRole("button", { name: "Use original scheduling choice" })
      .click();
    await page.getByRole("button", { name: "Stop original schedule" }).click();
    await expect(page.getByText(/original schedule was stopped/)).toBeVisible();
    expect(await pending(page, f.key)).toBeNull();
    expect(
      JSON.parse(
        (await page.evaluate((key) => localStorage.getItem(key), key))!,
      ).scheduled_at,
    ).toBe(saved.scheduled_at);
  });
});

test("a corrupt saved choice requires an explicit time review and leaves foreign choices alone", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    const key = `beweft.schedule-correction:${f.owner.id}:${f.brand.id}:${f.post.id}`;
    const foreign = `beweft.schedule-correction:${crypto.randomUUID()}:${f.brand.id}:${f.post.id}`;
    const otherBrand = `beweft.schedule-correction:${f.owner.id}:${crypto.randomUUID()}:${f.post.id}`;
    await page.goto("/#drafts");
    await page.evaluate(
      ({ key, foreign, otherBrand }) => {
        localStorage.setItem(key, "{damaged");
        localStorage.setItem(foreign, "foreign choice");
        localStorage.setItem(otherBrand, "other brand choice");
      },
      { key, foreign, otherBrand },
    );
    await page.getByRole("button", { name: "Edit draft", exact: true }).click();
    await expect(
      page.getByText(/saved scheduling time could not be understood/),
    ).toBeVisible();
    await page.getByLabel("When should this go out?").selectOption("exact");
    await expect(
      page.getByRole("button", { name: "Schedule post" }),
    ).toBeDisabled();
    const chosen = localTime(5 * 86400_000);
    await page.getByLabel(/Date and time/).fill(chosen);
    await expect(
      page.getByRole("button", { name: "Schedule post" }),
    ).toBeEnabled();
    expect(await page.evaluate((k) => localStorage.getItem(k), key)).toBeNull();
    expect(await page.evaluate((k) => localStorage.getItem(k), foreign)).toBe(
      "foreign choice",
    );
    expect(
      await page.evaluate((k) => localStorage.getItem(k), otherBrand),
    ).toBe("other brand choice");
    await page.getByRole("button", { name: "Schedule post" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
  });
});

test("a saved choice with the wrong scope requires an explicit queue choice", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    const key = `beweft.schedule-correction:${f.owner.id}:${f.brand.id}:${f.post.id}`;
    await page.goto("/#drafts");
    await page.evaluate(
      ({ key, f }) => {
        localStorage.setItem(
          key,
          JSON.stringify({
            revision: crypto.randomUUID(),
            owner_id: f.owner.id,
            brand_id: crypto.randomUUID(),
            post_id: f.post.id,
            mode: "queue",
            scheduled_at: null,
          }),
        );
      },
      { key, f: { owner: f.owner, post: f.post } },
    );
    await page.getByRole("button", { name: "Edit draft", exact: true }).click();
    await expect(
      page.getByText(/saved scheduling time could not be understood/),
    ).toBeVisible();
    await expect(page.getByLabel("When should this go out?")).toHaveValue(
      "draft",
    );
    await page.getByLabel("When should this go out?").selectOption("queue");
    expect(
      await page.evaluate((key) => localStorage.getItem(key), key),
    ).toBeNull();
    await expect(
      page.getByRole("button", { name: "Add to queue" }),
    ).toBeEnabled();
  });
});

test("unreadable saved-time storage blocks scheduling until it can be checked", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    const key = `beweft.schedule-correction:${f.owner.id}:${f.brand.id}:${f.post.id}`;
    await page.goto("/#drafts");
    await page.evaluate((key) => {
      const get = Storage.prototype.getItem;
      (window as any).restoreCorrectionRead = () => {
        Storage.prototype.getItem = get;
      };
      Storage.prototype.getItem = function (item) {
        if (item === key) throw new Error("Correction storage unavailable");
        return get.call(this, item);
      };
    }, key);
    await page.getByRole("button", { name: "Edit draft", exact: true }).click();
    await expect(
      page.getByText(/saved scheduling time could not be read/),
    ).toBeVisible();
    await expect(page.getByLabel("When should this go out?")).toBeDisabled();
    await page.evaluate(() => (window as any).restoreCorrectionRead());
    await page.getByRole("button", { name: "Check saved time again" }).click();
    await expect(page.getByLabel("When should this go out?")).toBeEnabled();
    await page.getByLabel("When should this go out?").selectOption("exact");
    await page.getByLabel(/Date and time/).fill(localTime(7 * 86400_000));
    await page.getByRole("button", { name: "Schedule post" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
  });
});

test("a malformed sibling cannot discard a validated publication binding", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await twoChannelSetup(page, context, write);
    let originals: any[] = [];
    await page.route(routeFor(f.post.id), async (route) => {
      const response = await route.fetch();
      expect(response.ok(), await response.text()).toBe(true);
      originals = (await response.json()).data;
      await route.fulfill({
        json: { data: [originals[0], { ...originals[1], channel_name: null }] },
      });
    });
    await start(page);
    await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
      "incomplete",
    );
    const saved = await pending(page, f.key);
    expect(saved.observed_publications).toEqual(
      expect.arrayContaining(
        originals.map((row) => ({
          channel_id: row.channel_id,
          publication_id: row.id,
        })),
      ),
    );
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(page.getByText(/original request is confirmed/)).toBeVisible();
    expect(await pending(page, f.key)).toBeNull();
    expect(
      await read(context, `/publications?brand_id=${f.brand.id}&view=summary`),
    ).toHaveLength(2);
  });
});

test("an incomplete receipt envelope cannot discard a validated GET observation", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await twoChannelSetup(page, context, write);
    let originals: any[] = [],
      reads = 0;
    await page.route(routeFor(f.post.id), async (route) => {
      const response = await route.fetch();
      expect(response.ok(), await response.text()).toBe(true);
      originals = (await response.json()).data;
      await route.abort("failed");
    });
    await page.route("**/api/v1/schedule-requests/*", async (route) => {
      const response = await route.fetch();
      expect(response.ok(), await response.text()).toBe(true);
      const data = (await response.json()).data;
      if (++reads === 1) {
        data.publications = [data.publications[0]];
        data.request = null;
      } else {
        data.outcome = "rejected";
        data.error = { code: "schedule_request_retired", message: "Stopped" };
        data.publications = [];
      }
      await route.fulfill({ json: { data } });
    });
    await start(page);
    await expect(
      page.getByRole("button", { name: "Check scheduling outcome" }),
    ).toBeEnabled();
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
      "receipt was incomplete",
    );
    const saved = await pending(page, f.key);
    expect(saved.observed_publications).toEqual([
      { channel_id: originals[0].channel_id, publication_id: originals[0].id },
    ]);
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(
      page.getByRole("status").getByText(/server record does not match/),
    ).toBeVisible();
    expect((await pending(page, f.key)).observed_publications).toEqual(
      saved.observed_publications,
    );
    expect(
      await read(context, `/publications?brand_id=${f.brand.id}&view=summary`),
    ).toHaveLength(2);
  });
});

test("a partial successful POST is recorded after the composer closes", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await twoChannelSetup(page, context, write);
    const committed = deferred(),
      release = deferred();
    let originals: any[] = [];
    await page.route(routeFor(f.post.id), async (route) => {
      const response = await route.fetch();
      expect(response.ok(), await response.text()).toBe(true);
      originals = (await response.json()).data;
      committed.resolve();
      await release.promise;
      await route.fulfill({ json: { data: [originals[0]] } }).catch(() => {});
    });
    await page.route("**/api/v1/schedule-requests/*", async (route) => {
      const response = await route.fetch();
      expect(response.ok(), await response.text()).toBe(true);
      const data = (await response.json()).data;
      data.outcome = "rejected";
      data.error = { code: "schedule_request_retired", message: "Stopped" };
      data.publications = [];
      await route.fulfill({ json: { data } });
    });
    try {
      await start(page);
      await committed.promise;
      await page.getByRole("button", { name: "Close", exact: true }).click();
      release.resolve();
      await expect
        .poll(async () => (await pending(page, f.key))?.observed_publications)
        .toEqual([
          {
            channel_id: originals[0].channel_id,
            publication_id: originals[0].id,
          },
        ]);
      await page
        .getByRole("button", { name: "Review pending schedule" })
        .click();
      await page
        .getByRole("button", { name: "Check scheduling outcome" })
        .click();
      await expect(
        page.getByRole("status").getByText(/server record does not match/),
      ).toBeVisible();
      expect((await pending(page, f.key)).rejection_code).toBe(
        "schedule_request_conflict",
      );
      expect(
        await read(
          context,
          `/publications?brand_id=${f.brand.id}&view=summary`,
        ),
      ).toHaveLength(2);
    } finally {
      release.resolve();
    }
  });
});

test("a complete matching receipt safely resolves a partially observed POST", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await twoChannelSetup(page, context, write);
    let originals: any[] = [];
    await page.route(routeFor(f.post.id), async (route) => {
      const response = await route.fetch();
      expect(response.ok(), await response.text()).toBe(true);
      originals = (await response.json()).data;
      await route.fulfill({ json: { data: [originals[0]] } });
    });
    const minute = new Date();
    minute.setSeconds(0, 0);
    await start(page, localTime(7 * 86400_000, minute.getTime()));
    await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
      "incomplete",
    );
    const saved = await pending(page, f.key);
    expect(saved.observed_publications).toEqual([
      { channel_id: originals[0].channel_id, publication_id: originals[0].id },
    ]);
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(page.getByText(/original request is confirmed/)).toBeVisible();
    expect(await pending(page, f.key)).toBeNull();
    const rows = await read(
      context,
      `/publications?brand_id=${f.brand.id}&view=summary`,
    );
    expect(rows.map((row: any) => row.id).sort()).toEqual(
      originals.map((row: any) => row.id).sort(),
    );
    expect((await read(context, `/posts/${f.post.id}`)).version).toBe(
      saved.post.version,
    );
    expect(f.patches()).toBe(1);
  });
});

for (const swapped of [false, true]) {
  test(`legacy ID-only proof is independently verified before ${swapped ? "a swapped" : "a matching"} receipt`, async () => {
    await withWorkspace(async (page, context, write) => {
      const f = await twoChannelSetup(page, context, write);
      let originals: any[] = [],
        failSummary = !swapped,
        checks = 0;
      await page.route(routeFor(f.post.id), async (route) => {
        const response = await route.fetch();
        expect(response.ok(), await response.text()).toBe(true);
        originals = (await response.json()).data;
        await route.abort("failed");
      });
      await page.route(/\/api\/v1\/publications\?.*\bid=/, (route) => {
        checks++;
        if (failSummary && checks === 1)
          return route.fulfill({
            status: 503,
            json: {
              error: { code: "unavailable", message: "Summary unavailable" },
            },
          });
        return route.continue();
      });
      if (swapped) {
        await page.route("**/api/v1/schedule-requests/*", async (route) => {
          const response = await route.fetch();
          expect(response.ok(), await response.text()).toBe(true);
          const data = (await response.json()).data;
          const [first, second] = data.publications;
          [first.channel_id, second.channel_id] = [
            second.channel_id,
            first.channel_id,
          ];
          await route.fulfill({ json: { data } });
        });
      }
      await start(page);
      await expect(
        page.getByRole("button", { name: "Check scheduling outcome" }),
      ).toBeEnabled();
      await page.evaluate(
        ({ key, ids }) => {
          const saved = JSON.parse(localStorage.getItem(key)!);
          saved.observed_publication_ids = ids;
          delete saved.observed_publications;
          localStorage.setItem(key, JSON.stringify(saved));
        },
        { key: f.key, ids: originals.map((row) => row.id).sort() },
      );
      await page.reload();
      await page
        .getByRole("button", { name: "Review pending schedule" })
        .click();
      await page
        .getByRole("button", { name: "Check scheduling outcome" })
        .click();
      if (!swapped) {
        await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
          "Summary unavailable",
        );
        await expect
          .poll(
            async () =>
              (await pending(page, f.key)).observed_publications?.length,
          )
          .toBe(1);
        expect((await pending(page, f.key)).observed_publication_ids).toEqual(
          originals.map((row) => row.id).sort(),
        );
        failSummary = false;
        await page
          .getByRole("button", { name: "Check scheduling outcome" })
          .click();
        await expect(
          page.getByText(/original request is confirmed/),
        ).toBeVisible();
        expect(await pending(page, f.key)).toBeNull();
      } else {
        await expect(
          page.getByRole("status").getByText(/server record does not match/),
        ).toBeVisible();
        const saved = await pending(page, f.key);
        expect(saved.observed_publications).toEqual(
          expect.arrayContaining(
            originals.map((row) => ({
              channel_id: row.channel_id,
              publication_id: row.id,
            })),
          ),
        );
        expect(saved.rejection_code).toBe("schedule_request_conflict");
      }
      expect(checks).toBeGreaterThanOrEqual(2);
      expect(
        await read(
          context,
          `/publications?brand_id=${f.brand.id}&view=summary`,
        ),
      ).toHaveLength(2);
    });
  });
}
