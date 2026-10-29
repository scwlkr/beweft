import { expect, test, type BrowserContext, type Page } from "@playwright/test";
import { deferred, withWorkspace } from "./helpers";

type Fixture = Awaited<ReturnType<typeof setup>>;
async function setup(
  page: Page,
  context: BrowserContext,
  write: (path: string, body: unknown) => Promise<any>,
) {
  const brand = await write("/brands", {
    name: `Schedule recovery ${crypto.randomUUID()}`,
  });
  const channel = await write("/channels", {
    brand_id: brand.id,
    provider: "manual",
    name: "Recovery channel",
    timezone: "UTC",
  });
  const post = await write("/posts", {
    brand_id: brand.id,
    title: "Recovery draft",
    text: "Original content",
    media_ids: [],
    variants: [{ channel_id: channel.id }],
  });
  await context.addInitScript(
    (id) => localStorage.setItem("beweft.brand", id),
    brand.id,
  );
  let patches = 0;
  page.on("request", (request) => {
    if (
      request.method() === "PATCH" &&
      request.url().includes(`/api/v1/posts/${post.id}`)
    )
      patches++;
  });
  return { brand, channel, post, patches: () => patches };
}
async function startSchedule(page: Page) {
  await page.goto("/#drafts");
  await page.getByRole("button", { name: "Edit draft", exact: true }).click();
  await expect(page.getByLabel("Post text", { exact: true })).toBeEnabled();
  await page.getByLabel("When should this go out?").selectOption("exact");
  const future = new Date(Date.now() + 3 * 86400_000)
    .toISOString()
    .slice(0, 16);
  await page.getByLabel(/Date and time/).fill(future);
  await page
    .getByRole("button", { name: "Schedule post", exact: true })
    .click();
}
async function snapshot(context: BrowserContext, postId: string) {
  const postResponse = await context.request.get(`/api/v1/posts/${postId}`);
  expect(postResponse.ok()).toBe(true);
  return (await postResponse.json()).data;
}
async function summaries(context: BrowserContext, brandId: string) {
  const response = await context.request.get(
    `/api/v1/publications?brand_id=${brandId}&view=summary`,
  );
  expect(response.ok()).toBe(true);
  return (await response.json()).data as any[];
}
async function actualSchedule(
  context: BrowserContext,
  postId: string,
  body: any,
) {
  const origin = process.env.BEWEFT_TEST_URL!;
  const { request_id: _fixtureRequestId, ...legacyBody } = body;
  const response = await context.request.post(
    `/api/v1/posts/${postId}/schedule?view=summary`,
    {
      data: legacyBody,
      headers: { "X-Beweft-Request": "1", Origin: origin },
    },
  );
  expect(response.ok(), await response.text()).toBe(true);
  return (await response.json()).data as any[];
}
function receipt(f: Fixture, body: any, rows: any[]) {
  return {
    request_id: body.request_id,
    post_id: f.post.id,
    brand_id: f.brand.id,
    request: {
      version: body.version,
      channel_ids: [...body.channel_ids].sort(),
      scheduled_at: body.scheduled_at ?? null,
      media_versions: body.media_versions ?? null,
    },
    outcome: "accepted",
    error: null,
    publications: rows,
  };
}

for (const reopen of ["close", "reload"] as const) {
  test(`in-flight 404 and ${reopen} preserve the original request for explicit retry (controlled request-ID fixture)`, async () => {
    await withWorkspace(async (page, context, write) => {
      const f = await setup(page, context, write);
      const first = deferred(),
        retry = deferred(),
        release = deferred(),
        committed = deferred();
      let committedReceipt: ReturnType<typeof receipt> | undefined;
      const requests: any[] = [];
      await page.route(
        /\/api\/v1\/posts\/[^/]+\/schedule\?view=summary$/,
        async (route) => {
          const body = route.request().postDataJSON();
          requests.push(body);
          if (requests.length === 1) {
            first.resolve();
            await release.promise;
            try {
              const rows = await actualSchedule(context, f.post.id, body);
              committedReceipt = receipt(f, body, rows);
            } finally {
              committed.resolve();
            }
            await route.abort("failed").catch(() => {});
          } else {
            retry.resolve();
            await committed.promise;
            if (!committedReceipt) throw new Error("Fixture commit failed");
            await route.fulfill({
              json: { data: committedReceipt.publications },
            });
          }
        },
      );
      await page.route("**/api/v1/schedule-requests/*", (route) =>
        committedReceipt
          ? route.fulfill({ json: { data: committedReceipt } })
          : route.fulfill({
              status: 404,
              json: {
                error: { code: "not_found", message: "No request record" },
              },
            }),
      );
      try {
        await startSchedule(page);
        await first.promise;
        expect(f.patches()).toBe(1);
        const savedVersion = (await snapshot(context, f.post.id)).version;
        expect(requests[0].version).toBe(savedVersion);
        expect(requests[0].request_id).toMatch(/^[0-9a-f-]{36}$/);
        await expect(
          page.getByRole("button", { name: "Close", exact: true }),
        ).toBeEnabled();
        await page.getByRole("button", { name: "Close", exact: true }).click();
        if (reopen === "reload") await page.reload();
        await page
          .getByRole("button", { name: "Review pending schedule" })
          .click();
        await expect(
          page.getByLabel("Post text", { exact: true }),
        ).toBeDisabled();
        await page
          .getByRole("button", { name: "Check scheduling outcome" })
          .click();
        await expect(
          page.getByText(/No request record is visible yet/),
        ).toBeVisible();
        await expect(
          page.getByRole("button", { name: "Retry original schedule" }),
        ).toBeEnabled();
        expect(await summaries(context, f.brand.id)).toHaveLength(0);
        await page
          .getByRole("button", { name: "Retry original schedule" })
          .click();
        await retry.promise;
        expect(requests[1]).toEqual(requests[0]);
        release.resolve();
        await expect(
          page.getByText(/The original request is confirmed/),
        ).toBeVisible();
        expect((await snapshot(context, f.post.id)).version).toBe(savedVersion);
        expect(await summaries(context, f.brand.id)).toHaveLength(1);
        expect(f.patches()).toBe(1);
      } finally {
        release.resolve();
      }
    });
  });
}

test("a lost response recovers the original canceled publication after replacement (controlled request-ID fixture)", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    const committed = deferred();
    let original: ReturnType<typeof receipt> | undefined,
      sends = 0;
    await page.route(
      /\/api\/v1\/posts\/[^/]+\/schedule\?view=summary$/,
      async (route) => {
        sends++;
        const body = route.request().postDataJSON();
        const rows = await actualSchedule(context, f.post.id, body);
        original = receipt(f, body, rows);
        committed.resolve();
        await route.abort("failed");
      },
    );
    await page.route("**/api/v1/schedule-requests/*", async (route) => {
      if (!original) throw new Error("Fixture has no committed request");
      const current = await summaries(context, f.brand.id);
      const ids = new Set(original.publications.map((row) => row.id));
      await route.fulfill({
        json: {
          data: {
            ...original,
            publications: current.filter((row) => ids.has(row.id)),
          },
        },
      });
    });
    await startSchedule(page);
    await committed.promise;
    await expect(
      page.getByRole("button", { name: "Close", exact: true }),
    ).toBeEnabled();
    const old = original!.publications[0];
    const current = await snapshot(context, f.post.id);
    const origin = process.env.BEWEFT_TEST_URL!;
    const revised = await context.request.patch(`/api/v1/posts/${f.post.id}`, {
      data: { version: current.version, text: "A later revision" },
      headers: { "X-Beweft-Request": "1", Origin: origin },
    });
    expect(revised.ok(), await revised.text()).toBe(true);
    const later = (await revised.json()).data;
    const replaced = await context.request.post(
      `/api/v1/publications/${old.id}/replace`,
      {
        data: {
          post_version: later.version,
          publication_post_version: old.post_version,
          scheduled_at: old.scheduled_at,
        },
        headers: { "X-Beweft-Request": "1", Origin: origin },
      },
    );
    expect(replaced.ok(), await replaced.text()).toBe(true);
    const replacement = (await replaced.json()).data.publication;
    await page.reload();
    await page.getByRole("button", { name: "Review pending schedule" }).click();
    await expect(page.getByLabel("Post text", { exact: true })).toHaveValue(
      "Original content",
    );
    await expect(page.getByLabel("Post text", { exact: true })).toBeDisabled();
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(page.getByText(/1 item is now canceled/)).toBeVisible();
    const all = await summaries(context, f.brand.id);
    expect(all).toHaveLength(2);
    expect(all.find((row) => row.id === old.id)?.state).toBe("canceled");
    expect(all.find((row) => row.id === replacement.id)?.state).toBe("queued");
    expect((await snapshot(context, f.post.id)).version).toBe(later.version);
    expect(sends).toBe(1);
    expect(f.patches()).toBe(1);
  });
});

for (const status of [400, 403, 404, 422]) {
  test(`a generic first ${status} and missing receipt remain unresolved`, async () => {
    await withWorkspace(async (page, context, write) => {
      const f = await setup(page, context, write);
      let sends = 0,
        reads = 0;
      await page.route(
        /\/api\/v1\/posts\/[^/]+\/schedule\?view=summary$/,
        (route) => {
          sends++;
          return route.fulfill({
            status,
            json: {
              error: {
                code: "invalid_request",
                message: "Fixture rejected schedule",
              },
            },
          });
        },
      );
      await page.route("**/api/v1/schedule-requests/*", (route) => {
        reads++;
        return route.fulfill({
          status: 404,
          json: { error: { code: "not_found", message: "No record" } },
        });
      });
      await startSchedule(page);
      await expect(page.getByText(/Fixture rejected schedule/)).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Retry original schedule" }),
      ).toBeEnabled();
      await page
        .getByRole("button", { name: "Check scheduling outcome" })
        .click();
      await expect(
        page.getByText(/No request record is visible yet/),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Stop original schedule" }),
      ).toBeEnabled();
      await expect(
        page.getByLabel("Post text", { exact: true }),
      ).toBeDisabled();
      expect(reads).toBe(1);
      expect(sends).toBe(1);
      expect(f.patches()).toBe(1);
      expect(await summaries(context, f.brand.id)).toHaveLength(0);
    });
  });
}

test("a later rejection cannot erase a previously uncertain request", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    const requests: any[] = [];
    await page.route(
      /\/api\/v1\/posts\/[^/]+\/schedule\?view=summary$/,
      (route) => {
        requests.push(route.request().postDataJSON());
        return requests.length === 1
          ? route.abort("failed")
          : route.fulfill({
              status: 409,
              json: {
                error: {
                  code: "post_version_conflict",
                  message: "Fixture rejected retry",
                },
              },
            });
      },
    );
    await page.route("**/api/v1/schedule-requests/*", (route) =>
      route.fulfill({
        status: 404,
        json: { error: { code: "not_found", message: "No record yet" } },
      }),
    );
    await startSchedule(page);
    await expect(
      page.getByRole("button", { name: "Retry original schedule" }),
    ).toBeEnabled();
    await page.getByRole("button", { name: "Retry original schedule" }).click();
    await expect(page.getByText(/Fixture rejected retry/)).toBeVisible();
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(
      page.getByText(/No request record is visible yet/),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Stop original schedule" }),
    ).toBeEnabled();
    await expect(page.getByLabel("Post text", { exact: true })).toBeDisabled();
    expect(requests).toHaveLength(2);
    expect(requests[1]).toEqual(requests[0]);
    expect(f.patches()).toBe(1);
    expect(await summaries(context, f.brand.id)).toHaveLength(0);
    await page.getByRole("button", { name: "Close", exact: true }).click();
    await page.reload();
    await expect(
      page.getByRole("button", { name: "Review pending schedule" }),
    ).toBeVisible();
  });
});

test("partial and mismatched request receipts keep the original intent locked", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    let sent: any,
      mismatch = false;
    await page.route(
      /\/api\/v1\/posts\/[^/]+\/schedule\?view=summary$/,
      (route) => {
        sent = route.request().postDataJSON();
        return route.abort("failed");
      },
    );
    await page.route("**/api/v1/schedule-requests/*", (route) => {
      const record = receipt(f, sent, []);
      if (mismatch) record.request.version += 1;
      return route.fulfill({ json: { data: record } });
    });
    await startSchedule(page);
    await expect(
      page.getByRole("button", { name: "Check scheduling outcome" }),
    ).toBeEnabled();
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(page.getByText(/only part of this request/)).toBeVisible();
    await expect(page.getByLabel("Post text", { exact: true })).toBeDisabled();
    await expect(
      page.getByRole("button", { name: "Retry original schedule" }),
    ).toBeEnabled();
    mismatch = true;
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(
      page.getByRole("status").getByText(/server record does not match/),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Retry original schedule" }),
    ).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: "Stop original schedule" }),
    ).toHaveCount(0);
    expect(f.patches()).toBe(1);
    expect(await summaries(context, f.brand.id)).toHaveLength(0);
    await page.getByRole("button", { name: "Close", exact: true }).click();
    await page.reload();
    await expect(
      page.getByRole("button", { name: "Review pending schedule" }),
    ).toBeVisible();
  });
});

test("a legacy unkeyed schedule is never retried as a new request", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    let sends = 0,
      lookups = 0;
    await page.route(
      /\/api\/v1\/posts\/[^/]+\/schedule\?view=summary$/,
      (route) => {
        sends++;
        return route.abort("failed");
      },
    );
    await page.route("**/api/v1/schedule-requests/*", (route) => {
      lookups++;
      return route.abort("failed");
    });
    const owner = (await (await context.request.get("/api/v1/auth/me")).json())
      .data;
    await page.goto("/#drafts");
    await page.evaluate(
      ({ ownerId, brandId, post, channelId }) => {
        localStorage.setItem(
          `beweft.pending-schedule:${ownerId}:${brandId}`,
          JSON.stringify({
            id: crypto.randomUUID(),
            post,
            channel_ids: [channelId],
            mode: "queue",
            uncertain: true,
          }),
        );
      },
      {
        ownerId: owner.id,
        brandId: f.brand.id,
        post: f.post,
        channelId: f.channel.id,
      },
    );
    await page.reload();
    await page.getByRole("button", { name: "Review pending schedule" }).click();
    await expect(
      page.getByText(/older request has no recovery ID/),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Retry original schedule" }),
    ).toHaveCount(0);
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(
      page.getByText(/older request was sent without a recovery ID/),
    ).toBeVisible();
    expect(sends).toBe(0);
    expect(lookups).toBe(0);
    expect(f.patches()).toBe(0);
  });
});

test("a late abandoned failure cannot overwrite a newer conflict across reload", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    const first = deferred(),
      retry = deferred(),
      releaseFirst = deferred(),
      releaseRetry = deferred();
    const requests: any[] = [];
    await page.route(
      /\/api\/v1\/posts\/[^/]+\/schedule\?view=summary$/,
      async (route) => {
        requests.push(route.request().postDataJSON());
        if (requests.length === 1) {
          first.resolve();
          await releaseFirst.promise;
          await route
            .fulfill({
              status: 503,
              json: {
                error: { code: "unavailable", message: "Old request failed" },
              },
            })
            .catch(() => {});
        } else {
          retry.resolve();
          await releaseRetry.promise;
          await route.fulfill({
            status: 409,
            json: {
              error: {
                code: "schedule_request_conflict",
                message: "Retry belongs to another schedule",
              },
            },
          });
        }
      },
    );
    await page.route("**/api/v1/schedule-requests/*", (route) =>
      route.fulfill({
        status: 404,
        json: { error: { code: "not_found", message: "No record yet" } },
      }),
    );
    try {
      await startSchedule(page);
      await first.promise;
      await expect(
        page.getByRole("button", { name: "Close", exact: true }),
      ).toBeEnabled();
      await page.getByRole("button", { name: "Close", exact: true }).click();
      await page
        .getByRole("button", { name: "Review pending schedule" })
        .click();
      await page
        .getByRole("button", { name: "Retry original schedule" })
        .click();
      await retry.promise;
      releaseRetry.resolve();
      await expect(
        page.getByText(/server record does not match/),
      ).toBeVisible();
      await page.evaluate(() => {
        const state = window as any;
        let release!: () => void, held!: () => void;
        const wait = new Promise<void>((resolve) => (release = resolve));
        state.authHeld = new Promise<void>((resolve) => (held = resolve));
        state.releaseAuth = release;
        void navigator.locks.request("beweft:authentication", async () => {
          held();
          await wait;
        });
      });
      await page.evaluate(() => (window as any).authHeld);
      releaseFirst.resolve();
      await expect
        .poll(() =>
          page.evaluate(async () =>
            (await navigator.locks.query()).pending.some(
              (lock) => lock.name === "beweft:authentication",
            ),
          ),
        )
        .toBe(true);
      await page.evaluate(async () => {
        (window as any).releaseAuth();
        await navigator.locks.request("beweft:authentication", () => {});
      });
      await expect
        .poll(() =>
          page.evaluate(() => {
            const entry = Object.entries(localStorage).find(([key]) =>
              key.startsWith("beweft.pending-schedule:"),
            );
            return entry ? JSON.parse(entry[1]).rejection_code : null;
          }),
        )
        .toBe("schedule_request_conflict");
      const before = await page.evaluate(() =>
        JSON.parse(
          Object.entries(localStorage).find(([key]) =>
            key.startsWith("beweft.pending-schedule:"),
          )![1],
        ),
      );
      expect(before.uncertain).toBe(true);
      await page.reload();
      await page
        .getByRole("button", { name: "Review pending schedule" })
        .click();
      await expect(
        page.getByText(/server record does not match/),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Retry original schedule" }),
      ).toHaveCount(0);
      expect(requests[1]).toEqual(requests[0]);
      expect(f.patches()).toBe(1);
    } finally {
      await page
        .evaluate(() => (window as any).releaseAuth?.())
        .catch(() => {});
      releaseFirst.resolve();
      releaseRetry.resolve();
    }
  });
});

for (const changed of ["revision", "snapshot"] as const) {
  test(`a held receipt cannot clear the same ID with a changed saved ${changed}`, async () => {
    await withWorkspace(async (page, context, write) => {
      const f = await setup(page, context, write);
      const owner = (
        await (await context.request.get("/api/v1/auth/me")).json()
      ).data;
      const key = `beweft.pending-schedule:${owner.id}:${f.brand.id}`;
      const held = deferred(),
        release = deferred();
      await page.route(
        /\/api\/v1\/posts\/[^/]+\/schedule\?view=summary$/,
        async (route) => {
          const response = await route.fetch();
          expect(response.ok(), await response.text()).toBe(true);
          await route.abort("failed");
        },
      );
      await page.route("**/api/v1/schedule-requests/*", async (route) => {
        const response = await route.fetch();
        expect(response.ok(), await response.text()).toBe(true);
        const payload = await response.json();
        payload.data.outcome = "accepted";
        payload.data.error = null;
        held.resolve();
        await release.promise;
        await route.fulfill({ json: payload }).catch(() => {});
      });
      const other = await context.newPage();
      try {
        await other.goto("/#drafts");
        await startSchedule(page);
        await expect(
          page.getByRole("button", { name: "Check scheduling outcome" }),
        ).toBeEnabled();
        await page
          .getByRole("button", { name: "Check scheduling outcome" })
          .click();
        await held.promise;
        const changedRecord = await other.evaluate(
          ({ key, changed }) => {
            const saved = JSON.parse(localStorage.getItem(key)!);
            if (changed === "revision") saved.post.version += 1;
            else saved.post.text = "Different saved snapshot";
            localStorage.setItem(key, JSON.stringify(saved));
            return saved;
          },
          { key, changed },
        );
        release.resolve();
        await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
          "saved schedule changed",
        );
        const stored = await other.evaluate(
          (key) => JSON.parse(localStorage.getItem(key)!),
          key,
        );
        expect(stored).toEqual(changedRecord);
        expect(await summaries(context, f.brand.id)).toHaveLength(1);
      } finally {
        release.resolve();
        await other.close();
      }
    });
  });
}

test("a schedule queued behind its lock cannot save or send after another tab signs out", async () => {
  await withWorkspace(async (old, context, write) => {
    await old.addInitScript(() => {
      window.addEventListener(
        "storage",
        (event) => {
          if (event.key === "beweft.auth-epoch")
            event.stopImmediatePropagation();
        },
        true,
      );
    });
    const f = await setup(old, context, write);
    const owner = (await (await context.request.get("/api/v1/auth/me")).json())
      .data;
    const key = `beweft.pending-schedule:${owner.id}:${f.brand.id}`;
    let sends = 0;
    await context.route(
      /\/api\/v1\/posts\/[^/]+\/schedule\?view=summary$/,
      (route) => {
        sends++;
        return route.abort("failed");
      },
    );
    const other = await context.newPage();
    try {
      await other.goto("/#drafts");
      await expect(
        other.getByRole("button", { name: "Sign out" }),
      ).toBeVisible();
      await other.evaluate((lockKey) => {
        const state = window as any;
        let release!: () => void, held!: () => void;
        const wait = new Promise<void>((resolve) => (release = resolve));
        state.heldScheduleLock = new Promise<void>(
          (resolve) => (held = resolve),
        );
        state.releaseScheduleLock = release;
        void navigator.locks.request(lockKey, async () => {
          held();
          await wait;
        });
      }, key);
      await other.evaluate(() => (window as any).heldScheduleLock);
      await startSchedule(old);
      await expect
        .poll(() =>
          old.evaluate(
            async (lockKey) =>
              (await navigator.locks.query()).pending.some(
                (lock) => lock.name === lockKey,
              ),
            key,
          ),
        )
        .toBe(true);
      const logout = other.waitForResponse((response) =>
        response.url().endsWith("/api/v1/auth/logout"),
      );
      await other.getByRole("button", { name: "Sign out" }).click();
      expect((await logout).status()).toBe(200);
      expect((await context.request.get("/api/v1/auth/me")).status()).toBe(401);
      await other.evaluate(() => (window as any).releaseScheduleLock());
      await expect(old.getByLabel("Email address")).toBeVisible();
      expect(
        await other.evaluate((lockKey) => localStorage.getItem(lockKey), key),
      ).toBeNull();
      expect(sends).toBe(0);
      expect(f.patches()).toBe(0);
      await other
        .getByLabel("Email address")
        .fill(process.env.BEWEFT_TEST_EMAIL!);
      await other
        .getByLabel("Password", { exact: true })
        .fill(process.env.BEWEFT_TEST_PASSWORD!);
      await other.getByRole("button", { name: "Sign in", exact: true }).click();
      await expect(other.getByLabel("Current brand")).toBeVisible();
      // Reload performs a new fenced owner check with the replacement cookie.
      await old.reload();
      await old
        .getByRole("button", { name: "Edit draft", exact: true })
        .click();
      await expect(old.getByLabel("Post text", { exact: true })).toBeEnabled();
      expect((await snapshot(context, f.post.id)).version).toBe(f.post.version);
    } finally {
      await other
        .evaluate(() => (window as any).releaseScheduleLock?.())
        .catch(() => {});
      await other.close();
    }
  });
});

test("a stale delete confirmation cannot remove a draft with a pending schedule", async () => {
  await withWorkspace(async (old, context, write) => {
    await old.addInitScript(() => {
      window.addEventListener(
        "storage",
        (event) => {
          if (event.key?.startsWith("beweft.pending-schedule:"))
            event.stopImmediatePropagation();
        },
        true,
      );
    });
    const f = await setup(old, context, write);
    const owner = (await (await context.request.get("/api/v1/auth/me")).json())
      .data;
    const key = `beweft.pending-schedule:${owner.id}:${f.brand.id}`;
    const started = deferred(),
      release = deferred();
    let deletes = 0;
    await old.route(new RegExp(`/api/v1/posts/${f.post.id}$`), (route) => {
      if (route.request().method() === "DELETE") deletes++;
      return route.continue();
    });
    await context.route(
      /\/api\/v1\/posts\/[^/]+\/schedule\?view=summary$/,
      async (route) => {
        started.resolve();
        await release.promise;
        await route.abort("failed").catch(() => {});
      },
    );
    const other = await context.newPage();
    try {
      await old.goto("/#drafts");
      await old.getByRole("button", { name: "Delete Recovery draft" }).click();
      await expect(
        old.getByRole("button", { name: "Delete draft", exact: true }),
      ).toBeEnabled();
      await startSchedule(other);
      await started.promise;
      await old
        .getByRole("button", { name: "Delete draft", exact: true })
        .click();
      await expect
        .poll(() =>
          old.evaluate(
            async (lockKey) =>
              (await navigator.locks.query()).pending.some(
                (lock) => lock.name === lockKey,
              ),
            key,
          ),
        )
        .toBe(true);
      release.resolve();
      await expect(
        old
          .getByRole("dialog")
          .getByText(/unresolved schedule. Check it before deleting/),
      ).toBeVisible();
      expect(deletes).toBe(0);
      expect((await snapshot(context, f.post.id)).id).toBe(f.post.id);
    } finally {
      release.resolve();
      await other.close();
    }
  });
});

test("a silently discarded schedule storage write cannot send a request", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    let sends = 0;
    page.on("request", (request) => {
      if (
        request.method() === "POST" &&
        new URL(request.url()).pathname.endsWith("/schedule")
      )
        sends++;
    });
    await page.goto("/#drafts");
    await page.getByRole("button", { name: "Edit draft", exact: true }).click();
    await page.getByLabel("When should this go out?").selectOption("exact");
    await page
      .getByLabel(/Date and time/)
      .fill(new Date(Date.now() + 3 * 86400_000).toISOString().slice(0, 16));
    await expect(
      page.getByRole("button", { name: "Schedule post" }),
    ).toBeEnabled();
    await page.evaluate(() => {
      const original = Storage.prototype.setItem;
      (window as any).restoreScheduleStorage = () => {
        Storage.prototype.setItem = original;
      };
      Storage.prototype.setItem = function (key, value) {
        if (this === localStorage && key.startsWith("beweft.pending-schedule:"))
          return;
        return original.call(this, key, value);
      };
    });
    try {
      await page.getByRole("button", { name: "Schedule post" }).click();
      await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
        "could not be saved in browser storage",
      );
      expect(sends).toBe(0);
      expect(await summaries(context, f.brand.id)).toEqual([]);
    } finally {
      await page.evaluate(() => (window as any).restoreScheduleStorage());
    }
  });
});

for (const mismatch of ["microseconds", "media map"] as const) {
  test(`a receipt with mismatched ${mismatch} remains locked`, async () => {
    await withWorkspace(async (page, context, write) => {
      const f = await setup(page, context, write);
      let sent: any;
      await page.route(
        /\/api\/v1\/posts\/[^/]+\/schedule\?view=summary$/,
        (route) => {
          sent = route.request().postDataJSON();
          return route.abort("failed");
        },
      );
      await page.route("**/api/v1/schedule-requests/*", (route) => {
        const record = receipt(f, sent, []);
        if (mismatch === "microseconds")
          record.request.scheduled_at = sent.scheduled_at.replace(
            /\.(\d{3})Z$/,
            ".$1001Z",
          );
        else record.request.media_versions = { [crypto.randomUUID()]: 1 };
        return route.fulfill({ json: { data: record } });
      });
      await startSchedule(page);
      await page
        .getByRole("button", { name: "Check scheduling outcome" })
        .click();
      await expect(
        page.getByText(/server record does not match/),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Retry original schedule" }),
      ).toHaveCount(0);
      await expect(
        page.getByLabel("Post text", { exact: true }),
      ).toBeDisabled();
      expect(await summaries(context, f.brand.id)).toEqual([]);
    });
  });
}

test("a conflicting request ID stays locked after reload without a replacement request", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    let sends = 0;
    await page.route(
      /\/api\/v1\/posts\/[^/]+\/schedule\?view=summary$/,
      (route) => {
        sends++;
        return route.fulfill({
          status: 409,
          json: {
            error: {
              code: "schedule_request_conflict",
              message: "The request belongs to another schedule",
            },
          },
        });
      },
    );
    await startSchedule(page);
    await expect(page.getByText(/server record does not match/)).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Retry original schedule" }),
    ).toHaveCount(0);
    await page.reload();
    await page.getByRole("button", { name: "Review pending schedule" }).click();
    await expect(page.getByText(/server record does not match/)).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Retry original schedule" }),
    ).toHaveCount(0);
    expect(sends).toBe(1);
    expect(f.patches()).toBe(1);
    expect(await summaries(context, f.brand.id)).toEqual([]);
  });
});

test("a receipt with different original IDs cannot replace already observed publications", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    let sent: any;
    let original: any[] = [];
    await page.route(
      /\/api\/v1\/posts\/[^/]+\/schedule\?view=summary$/,
      async (route) => {
        sent = route.request().postDataJSON();
        const response = await route.fetch();
        expect(response.ok(), await response.text()).toBe(true);
        original = (await response.json()).data;
        await route.fulfill({ response });
      },
    );
    await page.route("**/api/v1/schedule-requests/*", (route) => {
      const altered = [{ ...original[0], id: crypto.randomUUID() }];
      return route.fulfill({ json: { data: receipt(f, sent, altered) } });
    });
    await startSchedule(page);
    await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
      "server record does not match",
    );
    const stored = await page.evaluate(() => {
      const entry = Object.entries(localStorage).find(([key]) =>
        key.startsWith("beweft.pending-schedule:"),
      );
      return entry ? JSON.parse(entry[1]) : null;
    });
    expect(stored.uncertain).toBe(true);
    expect(stored.observed_publication_ids).toEqual(
      original.map((row) => row.id),
    );
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(page.getByText(/server record does not match/)).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Retry original schedule" }),
    ).toHaveCount(0);
    expect((await summaries(context, f.brand.id)).map((row) => row.id)).toEqual(
      original.map((row) => row.id),
    );
    expect(f.patches()).toBe(1);
  });
});

test("a held retirement receipt cannot clear a changed same-ID snapshot", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    const owner = (await (await context.request.get("/api/v1/auth/me")).json())
      .data;
    const key = `beweft.pending-schedule:${owner.id}:${f.brand.id}`;
    const held = deferred(),
      release = deferred();
    let sent: any;
    await page.route(
      /\/api\/v1\/posts\/[^/]+\/schedule\?view=summary$/,
      (route) => {
        sent = route.request().postDataJSON();
        return route.abort("failed");
      },
    );
    await page.route("**/api/v1/schedule-requests/*/retire", async (route) => {
      const result = {
        ...receipt(f, sent, []),
        outcome: "rejected",
        error: { code: "schedule_request_retired", message: "Stopped" },
      };
      held.resolve();
      await release.promise;
      await route.fulfill({ json: { data: result } }).catch(() => {});
    });
    const other = await context.newPage();
    try {
      await other.goto("/#drafts");
      await startSchedule(page);
      await page
        .getByRole("button", { name: "Stop original schedule" })
        .click();
      await held.promise;
      const updated = await other.evaluate((key) => {
        const saved = JSON.parse(localStorage.getItem(key)!);
        saved.post.text = "Changed during retirement";
        localStorage.setItem(key, JSON.stringify(saved));
        return saved;
      }, key);
      release.resolve();
      await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
        "saved schedule changed",
      );
      expect(
        await other.evaluate(
          (key) => JSON.parse(localStorage.getItem(key)!),
          key,
        ),
      ).toEqual(updated);
      expect(await summaries(context, f.brand.id)).toEqual([]);
    } finally {
      release.resolve();
      await other.close();
    }
  });
});

test("a matching rejected receipt unlocks correction and a deliberate new request", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    const sent: any[] = [];
    await page.route(
      /\/api\/v1\/posts\/[^/]+\/schedule\?view=summary$/,
      (route) => {
        sent.push(route.request().postDataJSON());
        return sent.length === 1
          ? route.fulfill({
              status: 400,
              json: {
                error: {
                  code: "provider_format_invalid",
                  message: "Text is too long",
                },
              },
            })
          : route.continue();
      },
    );
    await page.route("**/api/v1/schedule-requests/*", async (route) => {
      if (route.request().url().endsWith(sent[0].request_id)) {
        const result = {
          ...receipt(f, sent[0], []),
          outcome: "rejected",
          error: {
            code: "provider_format_invalid",
            message: "Text is too long",
          },
        };
        return route.fulfill({ json: { data: result } });
      }
      const response = await route.fetch();
      expect(response.ok(), await response.text()).toBe(true);
      const payload = await response.json();
      payload.data.outcome = "accepted";
      payload.data.error = null;
      return route.fulfill({ json: payload });
    });
    await startSchedule(page);
    await expect(page.getByLabel("Post text", { exact: true })).toBeDisabled();
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(
      page.getByText(/original schedule was not made: Text is too long/),
    ).toBeVisible();
    await expect(page.getByLabel("Post text", { exact: true })).toBeEnabled();
    await page.getByLabel("Post text", { exact: true }).fill("Corrected text");
    await page.getByRole("button", { name: "Schedule post" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    expect(sent).toHaveLength(2);
    expect(sent[1].request_id).not.toBe(sent[0].request_id);
    expect(sent[1].version).toBe(sent[0].version + 1);
    expect(await summaries(context, f.brand.id)).toHaveLength(1);
    expect(f.patches()).toBe(2);
  });
});

test("a generic 401 retains the unresolved intent through reauthentication", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    const owner = (await (await context.request.get("/api/v1/auth/me")).json())
      .data;
    const key = `beweft.pending-schedule:${owner.id}:${f.brand.id}`;
    let sent: any;
    await page.route(
      /\/api\/v1\/posts\/[^/]+\/schedule\?view=summary$/,
      (route) => {
        sent = route.request().postDataJSON();
        return route.fulfill({
          status: 401,
          json: { error: { code: "unauthorized", message: "Session expired" } },
        });
      },
    );
    await startSchedule(page);
    await expect(page.getByLabel("Email address")).toBeVisible();
    const saved = await page.evaluate(
      (key) => JSON.parse(localStorage.getItem(key)!),
      key,
    );
    expect(saved.id).toBe(sent.request_id);
    expect(saved.uncertain).toBe(true);
    await page.getByLabel("Email address").fill(process.env.BEWEFT_TEST_EMAIL!);
    await page
      .getByLabel("Password", { exact: true })
      .fill(process.env.BEWEFT_TEST_PASSWORD!);
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await page.getByRole("button", { name: "Review pending schedule" }).click();
    await expect(page.getByLabel("Post text", { exact: true })).toBeDisabled();
    await expect(
      page.getByRole("button", { name: "Retry original schedule" }),
    ).toBeEnabled();
    expect(
      (
        await page.evaluate(
          (key) => JSON.parse(localStorage.getItem(key)!),
          key,
        )
      ).id,
    ).toBe(saved.id);
    expect(f.patches()).toBe(1);
  });
});

test("a rejected receipt cannot erase publications already observed in a successful response", async () => {
  await withWorkspace(async (page, context, write) => {
    const f = await setup(page, context, write);
    let sent: any;
    let original: any[] = [];
    await page.route(
      /\/api\/v1\/posts\/[^/]+\/schedule\?view=summary$/,
      async (route) => {
        sent = route.request().postDataJSON();
        const response = await route.fetch();
        expect(response.ok(), await response.text()).toBe(true);
        original = (await response.json()).data;
        await route.fulfill({ response });
      },
    );
    await page.route("**/api/v1/schedule-requests/*", (route) => {
      const record = {
        ...receipt(f, sent, []),
        outcome: "rejected",
        error: { code: "schedule_request_retired", message: "Stopped" },
      };
      return route.fulfill({ json: { data: record } });
    });
    await startSchedule(page);
    await expect(
      page.getByRole("status").getByText(/server record does not match/),
    ).toBeVisible();
    const stored = await page.evaluate(() => {
      const entry = Object.entries(localStorage).find(([key]) =>
        key.startsWith("beweft.pending-schedule:"),
      );
      return entry ? JSON.parse(entry[1]) : null;
    });
    expect(stored.observed_publication_ids).toEqual(
      original.map((row) => row.id),
    );
    expect(stored.rejection_code).toBe("schedule_request_conflict");
    await page.reload();
    await page.getByRole("button", { name: "Review pending schedule" }).click();
    await expect(
      page.getByRole("status").getByText(/server record does not match/),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Stop original schedule" }),
    ).toHaveCount(0);
    expect((await summaries(context, f.brand.id)).map((row) => row.id)).toEqual(
      original.map((row) => row.id),
    );
  });
});
