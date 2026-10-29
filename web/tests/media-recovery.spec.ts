import {
  spawn,
  execFileSync,
  type ChildProcessWithoutNullStreams,
} from "node:child_process";
import { expect, test, type BrowserContext } from "@playwright/test";
import { deferred, withWorkspace } from "./helpers";

const pixel = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aL1sAAAAASUVORK5CYII=",
  "base64",
);
async function upload(context: BrowserContext, brand: string, name: string) {
  const result = await context.request.post(`/api/v1/media?brand_id=${brand}`, {
    multipart: { file: { name, mimeType: "image/png", buffer: pixel } },
    headers: { "X-Beweft-Request": "1", Origin: process.env.BEWEFT_TEST_URL! },
  });
  expect(result.ok(), await result.text()).toBe(true);
  return (await result.json()).data;
}
async function asset(context: BrowserContext, id: string) {
  const result = await context.request.get(`/api/v1/media/${id}`);
  expect(result.ok(), await result.text()).toBe(true);
  return (await result.json()).data;
}
async function update(
  context: BrowserContext,
  id: string,
  version: number,
  text: string,
) {
  const result = await context.request.patch(`/api/v1/media/${id}`, {
    data: { metadata_version: version, alt_text: text },
    headers: { "X-Beweft-Request": "1", Origin: process.env.BEWEFT_TEST_URL! },
  });
  expect(result.ok(), await result.text()).toBe(true);
  return (await result.json()).data;
}

// Hold an actual PostgreSQL row lock so the real server receives the PATCH but
// cannot commit. Ordinary GETs still see its previous committed version.
async function holdMediaRow(id: string) {
  if (!/^[a-f0-9-]{36}$/.test(id)) throw new Error("Invalid fixture media id");
  const database = process.env.BEWEFT_TEST_DB_NAME!;
  if (!/^beweft_e2e_[a-f0-9]{12}$/.test(database))
    throw new Error("Requires disposable database");
  const args = [
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
  ];
  const lockProcess: ChildProcessWithoutNullStreams = spawn("docker", args);
  let output = "";
  const ready = deferred();
  lockProcess.stdout.on("data", (chunk: Buffer) => {
    output += chunk.toString();
    if (output.includes("MEDIA_LOCKED")) ready.resolve();
  });
  lockProcess.stdin.write(
    `BEGIN; SELECT id FROM media WHERE id='${id}' FOR UPDATE; SELECT 'MEDIA_LOCKED';\n`,
  );
  await Promise.race([
    ready.promise,
    new Promise((_, reject) =>
      lockProcess.once("exit", () =>
        reject(new Error("Row lock process exited")),
      ),
    ),
  ]);
  return {
    waiting: () =>
      Number(
        execFileSync(
          "docker",
          [
            "exec",
            process.env.BEWEFT_TEST_POSTGRES_CONTAINER ?? "beweft-dev-postgres",
            "psql",
            "-U",
            process.env.BEWEFT_TEST_DB_USER ?? "beweft",
            "-d",
            database,
            "-Atq",
            "-c",
            "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'UPDATE media SET alt_text=%'",
          ],
          { encoding: "utf8" },
        ).trim(),
      ),
    release: async () => {
      if (lockProcess.exitCode !== null) return;
      lockProcess.stdin.end("COMMIT;\n");
      await new Promise<void>((resolve) =>
        lockProcess.once("exit", () => resolve()),
      );
    },
  };
}

test("a pre-commit save stays pending through navigation and a same-CAS retry fences it", async () => {
  await withWorkspace(async (page, context, write) => {
    const alpha = await write("/brands", { name: "Pending description Alpha" });
    const beta = await write("/brands", { name: "Pending description Beta" });
    const item = await upload(context, alpha.id, "delayed-alpha.png");
    await upload(context, beta.id, "delayed-beta.png");
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      alpha.id,
    );
    await page.goto("/#media");
    await page
      .locator(".media-grid")
      .getByRole("button", { name: /delayed-alpha\.png/ })
      .click();
    await page
      .getByLabel("Description", { exact: true })
      .fill("The first intent");
    const row = await holdMediaRow(item.id);
    try {
      await page.getByRole("button", { name: "Save description" }).click();
      await expect.poll(row.waiting).toBeGreaterThanOrEqual(1);
      expect((await asset(context, item.id)).metadata_version).toBe(1);
      await page.getByRole("button", { name: "Close dialog" }).click();
      await page
        .getByRole("button", { name: /^Drafts/ })
        .first()
        .click();
      await page
        .getByRole("button", { name: "Media library", exact: true })
        .click();
      await page
        .locator(".media-grid")
        .getByRole("button", { name: /delayed-alpha\.png/ })
        .click();
      await expect(page.getByLabel("Description", { exact: true })).toHaveValue(
        "The first intent",
      );
      await expect(
        page.getByLabel("Description", { exact: true }),
      ).toBeDisabled();
      await page.getByRole("button", { name: "Check description" }).click();
      await expect(
        page.getByRole("button", { name: "Retry this description" }),
      ).toBeVisible();
      await page.getByRole("button", { name: "Close dialog" }).click();
      await page.getByLabel("Current brand").selectOption(beta.id);
      await expect(
        page
          .locator(".media-grid")
          .getByRole("button", { name: /delayed-beta\.png/ }),
      ).toBeVisible();
      const failRead = (route: import("@playwright/test").Route) =>
        route.request().method() === "GET"
          ? route.fulfill({
              status: 503,
              json: {
                error: {
                  code: "unavailable",
                  message: "Asset read unavailable",
                },
              },
            })
          : route.continue();
      await page.route(`**/api/v1/media/${item.id}`, failRead);
      await page.getByLabel("Current brand").selectOption(alpha.id);
      await page
        .locator(".media-grid")
        .getByRole("button", { name: /delayed-alpha\.png/ })
        .click();
      await expect(
        page.getByLabel("Description", { exact: true }),
      ).toBeDisabled();
      await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
        "Asset read unavailable",
      );
      await page.unroute(`**/api/v1/media/${item.id}`, failRead);
      await page.getByRole("button", { name: "Check description" }).click();
      await page
        .getByRole("button", { name: "Retry this description" })
        .click();
      await expect.poll(row.waiting).toBeGreaterThanOrEqual(2);
      await row.release();
      await expect
        .poll(async () => (await asset(context, item.id)).metadata_version)
        .toBe(2);
      await expect(
        page.getByLabel("Description", { exact: true }),
      ).toBeEnabled();
      await expect(page.getByLabel("Description", { exact: true })).toHaveValue(
        "The first intent",
      );
      expect(await asset(context, item.id)).toMatchObject({
        metadata_version: 2,
        alt_text: "The first intent",
      });
    } finally {
      await row.release();
    }
  });
});

test("a lost committed response is recovered after reload without another write", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Lost description response" });
    const item = await upload(context, brand.id, "lost.png");
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    let writes = 0;
    await page.route(`**/api/v1/media/${item.id}`, async (route) => {
      if (route.request().method() !== "PATCH") return route.continue();
      writes++;
      const response = await route.fetch();
      expect(response.ok()).toBe(true);
      await route.abort("failed");
    });
    await page.goto("/#media");
    await page
      .locator(".media-grid")
      .getByRole("button", { name: /lost\.png/ })
      .click();
    await page
      .getByLabel("Description", { exact: true })
      .fill("Committed before response loss");
    await page.getByRole("button", { name: "Save description" }).click();
    await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
      "unconfirmed",
    );
    expect(await asset(context, item.id)).toMatchObject({
      metadata_version: 2,
      alt_text: "Committed before response loss",
    });
    await page.reload();
    await page
      .locator(".media-grid")
      .getByRole("button", { name: /lost\.png/ })
      .click();
    await expect(page.getByLabel("Description", { exact: true })).toBeEnabled();
    await expect(page.getByLabel("Description", { exact: true })).toHaveValue(
      "Committed before response loss",
    );
    expect(writes).toBe(1);
    expect(
      await page.evaluate(() =>
        Object.keys(localStorage).filter((key) =>
          key.startsWith("beweft.media-description:"),
        ),
      ),
    ).toEqual([]);
  });
});

test("failed post-save refresh blocks editing and does not announce saved", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", {
      name: "Description refresh failure",
    });
    const item = await upload(context, brand.id, "refresh.png");
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    let failReads = false;
    const handler = async (route: import("@playwright/test").Route) => {
      if (route.request().method() === "PATCH") {
        const response = await route.fetch();
        failReads = true;
        return route.fulfill({ response });
      }
      if (failReads && route.request().method() === "GET")
        return route.fulfill({
          status: 503,
          json: {
            error: {
              code: "unavailable",
              message: "Read temporarily unavailable",
            },
          },
        });
      return route.continue();
    };
    await page.route(`**/api/v1/media/${item.id}`, handler);
    await page.goto("/#media");
    await page
      .locator(".media-grid")
      .getByRole("button", { name: /refresh\.png/ })
      .click();
    await page
      .getByLabel("Description", { exact: true })
      .fill("Committed but not refreshed");
    await page.getByRole("button", { name: "Save description" }).click();
    await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
      "Read temporarily unavailable",
    );
    await expect(
      page.getByLabel("Description", { exact: true }),
    ).toBeDisabled();
    await expect(
      page.getByRole("dialog").getByText("Description saved."),
    ).toHaveCount(0);
    expect((await asset(context, item.id)).metadata_version).toBe(2);
    failReads = false;
    await page.getByRole("button", { name: "Check description" }).click();
    await expect(page.getByLabel("Description", { exact: true })).toBeEnabled();
    await expect(page.getByLabel("Description", { exact: true })).toHaveValue(
      "Committed but not refreshed",
    );
  });
});

test("a late old response cannot replace a newer description or its edits", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Late description response" });
    const item = await upload(context, brand.id, "late.png");
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    const held = deferred(),
      committed = deferred();
    let writes = 0;
    await page.route(`**/api/v1/media/${item.id}`, async (route) => {
      if (route.request().method() !== "PATCH") return route.continue();
      writes++;
      if (writes !== 1) return route.continue();
      const response = await route.fetch();
      committed.resolve();
      await held.promise;
      await route.fulfill({ response }).catch(() => {});
    });
    try {
      await page.goto("/#media");
      await page
        .locator(".media-grid")
        .getByRole("button", { name: /late\.png/ })
        .click();
      await page
        .getByLabel("Description", { exact: true })
        .fill("Older saved text");
      await page.getByRole("button", { name: "Save description" }).click();
      await committed.promise;
      await page.getByRole("button", { name: "Close dialog" }).click();
      await page
        .getByRole("button", { name: /^Drafts/ })
        .first()
        .click();
      await page
        .getByRole("button", { name: "Media library", exact: true })
        .click();
      await page
        .locator(".media-grid")
        .getByRole("button", { name: /late\.png/ })
        .click();
      await expect(page.getByLabel("Description", { exact: true })).toHaveValue(
        "Older saved text",
      );
      await page
        .getByLabel("Description", { exact: true })
        .fill("Newer saved text");
      await page.getByRole("button", { name: "Save description" }).click();
      await expect(page.getByRole("dialog").getByRole("status")).toHaveText(
        "Description saved.",
      );
      await page
        .getByLabel("Description", { exact: true })
        .fill("A newer unsaved edit");
      const delayed = page.waitForResponse(
        (response) =>
          response.request().method() === "PATCH" &&
          response.url().endsWith(`/api/v1/media/${item.id}`),
      );
      held.resolve();
      await (await delayed).finished();
      await page.evaluate(
        () =>
          new Promise<void>((resolve) =>
            requestAnimationFrame(() => resolve()),
          ),
      );
      await expect(page.getByLabel("Description", { exact: true })).toHaveValue(
        "A newer unsaved edit",
      );
      expect(await asset(context, item.id)).toMatchObject({
        metadata_version: 3,
        alt_text: "Newer saved text",
      });
    } finally {
      held.resolve();
    }
  });
});

test("external updates reject stale saves and refresh retains unsaved text until reviewed", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Description conflicts" });
    const item = await upload(context, brand.id, "conflict.png");
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.goto("/#media");
    await page
      .locator(".media-grid")
      .getByRole("button", { name: /conflict\.png/ })
      .click();
    await page
      .getByLabel("Description", { exact: true })
      .fill("My stale description");
    await update(context, item.id, 1, "External version");
    await page.getByRole("button", { name: "Save description" }).click();
    await expect(page.getByRole("dialog").getByRole("status")).toContainText(
      "unsaved text is kept",
    );
    await expect(page.getByLabel("Description", { exact: true })).toHaveValue(
      "My stale description",
    );
    await expect(
      page.getByText("Last loaded description: External version"),
    ).toBeVisible();
    await page.getByRole("button", { name: "Use latest description" }).click();
    await expect(page.getByLabel("Description", { exact: true })).toHaveValue(
      "External version",
    );
    await expect(page.getByLabel("Description", { exact: true })).toBeEnabled();
    expect(await asset(context, item.id)).toMatchObject({
      metadata_version: 2,
      alt_text: "External version",
    });
    await page
      .getByLabel("Description", { exact: true })
      .fill("Keep my unsaved text");
    await update(context, item.id, 2, "Another external version");
    await page
      .locator('[aria-label="Refresh workspace"]')
      .evaluate((button: HTMLElement) => button.click());
    await expect(page.getByRole("dialog").getByRole("status")).toContainText(
      "changed elsewhere",
    );
    await expect(page.getByLabel("Description", { exact: true })).toHaveValue(
      "Keep my unsaved text",
    );
    await expect(
      page.getByRole("button", { name: "Save description" }),
    ).toBeDisabled();
    await page.getByRole("button", { name: "Use latest description" }).click();
    await expect(page.getByLabel("Description", { exact: true })).toHaveValue(
      "Another external version",
    );
    await expect(page.getByLabel("Description", { exact: true })).toBeEnabled();
  });
});

test("another tab's pending edit preserves unsaved text and does not send it", async () => {
  await withWorkspace(async (first, context, write) => {
    const brand = await write("/brands", { name: "Concurrent media tabs" });
    const item = await upload(context, brand.id, "shared.png");
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await first.goto("/#media");
    const other = await context.newPage();
    await other.goto("/#media");
    await first
      .locator(".media-grid")
      .getByRole("button", { name: /shared\.png/ })
      .click();
    await other
      .locator(".media-grid")
      .getByRole("button", { name: /shared\.png/ })
      .click();
    await first
      .getByLabel("Description", { exact: true })
      .fill("First tab description");
    await other
      .getByLabel("Description", { exact: true })
      .fill("Other tab unsaved text");
    const row = await holdMediaRow(item.id);
    try {
      await first.getByRole("button", { name: "Save description" }).click();
      await expect.poll(row.waiting).toBeGreaterThanOrEqual(1);
      await expect(
        other
          .getByRole("dialog")
          .getByText("Pending description: First tab description"),
      ).toBeVisible();
      await expect(
        other.getByLabel("Description", { exact: true }),
      ).toHaveValue("Other tab unsaved text");
      await expect(
        other.getByRole("button", { name: "Save description" }),
      ).toBeDisabled();
      await row.release();
      await expect
        .poll(async () => (await asset(context, item.id)).metadata_version)
        .toBe(2);
      await expect(other.getByRole("dialog").getByRole("status")).toContainText(
        "unsaved text is kept",
      );
      await expect(
        other.getByLabel("Description", { exact: true }),
      ).toHaveValue("Other tab unsaved text");
      expect(await asset(context, item.id)).toMatchObject({
        metadata_version: 2,
        alt_text: "First tab description",
      });
    } finally {
      await row.release();
      await other.close();
    }
  });
});

test("a queued claim after sign-out creates no stale intent or media write", async () => {
  await withWorkspace(async (old, context, write) => {
    await old.addInitScript(() => {
      window.addEventListener(
        "storage",
        (event) => {
          if (event.key === "beweft.auth-epoch" || event.key === null)
            event.stopImmediatePropagation();
        },
        true,
      );
    });
    const brand = await write("/brands", { name: "Media claim session" });
    const item = await upload(context, brand.id, "session.png");
    const owner = (await (await context.request.get("/api/v1/auth/me")).json())
      .data;
    const key = `beweft.media-description:${owner.id}:${brand.id}:${item.id}`;
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await old.goto("/#media");
    const other = await context.newPage();
    await other.goto("/#media");
    await old
      .locator(".media-grid")
      .getByRole("button", { name: /session\.png/ })
      .click();
    await old
      .getByLabel("Description", { exact: true })
      .fill("Must not be sent");
    let writes = 0;
    old.on("request", (request) => {
      if (
        request.method() === "PATCH" &&
        request.url().endsWith(`/api/v1/media/${item.id}`)
      )
        writes++;
    });
    await other.evaluate((lockKey) => {
      const state = window as typeof window & {
        releaseMedia: () => void;
        heldMedia: Promise<void>;
      };
      let release!: () => void, held!: () => void;
      const wait = new Promise<void>((resolve) => {
        release = resolve;
      });
      state.heldMedia = new Promise<void>((resolve) => {
        held = resolve;
      });
      state.releaseMedia = release;
      void navigator.locks.request(lockKey, async () => {
        held();
        await wait;
      });
    }, key);
    await other.evaluate(
      () => (window as typeof window & { heldMedia: Promise<void> }).heldMedia,
    );
    try {
      await old.getByRole("button", { name: "Save description" }).click();
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
      await other.evaluate(() =>
        (window as typeof window & { releaseMedia: () => void }).releaseMedia(),
      );
      await expect(old.getByLabel("Email address")).toBeVisible();
      expect(
        await other.evaluate((lockKey) => localStorage.getItem(lockKey), key),
      ).toBeNull();
      expect(writes).toBe(0);
      await other
        .getByLabel("Email address")
        .fill(process.env.BEWEFT_TEST_EMAIL!);
      await other
        .getByLabel("Password", { exact: true })
        .fill(process.env.BEWEFT_TEST_PASSWORD!);
      await other.getByRole("button", { name: "Sign in", exact: true }).click();
      await other
        .locator(".media-grid")
        .getByRole("button", { name: /session\.png/ })
        .click();
      await expect(
        other.getByLabel("Description", { exact: true }),
      ).toBeEnabled();
      await other
        .getByLabel("Description", { exact: true })
        .fill("After sign-in");
      await other.getByRole("button", { name: "Save description" }).click();
      await expect(other.getByRole("dialog").getByRole("status")).toHaveText(
        "Description saved.",
      );
      expect(await asset(context, item.id)).toMatchObject({
        metadata_version: 2,
        alt_text: "After sign-in",
      });
    } finally {
      await other
        .evaluate(() =>
          (
            window as typeof window & { releaseMedia?: () => void }
          ).releaseMedia?.(),
        )
        .catch(() => {});
      await other.close();
    }
  });
});

test("an already-sent unknown edit survives sign-out and fences after reauthentication", async () => {
  await withWorkspace(async (old, context, write) => {
    const brand = await write("/brands", { name: "Unknown media session" });
    const item = await upload(context, brand.id, "unknown-session.png");
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await old.goto("/#media");
    const other = await context.newPage();
    await other.goto("/#media");
    await old
      .locator(".media-grid")
      .getByRole("button", { name: /unknown-session\.png/ })
      .click();
    await old
      .getByLabel("Description", { exact: true })
      .fill("Unknown across sessions");
    const row = await holdMediaRow(item.id);
    try {
      await old.getByRole("button", { name: "Save description" }).click();
      await expect.poll(row.waiting).toBeGreaterThanOrEqual(1);
      await other.getByRole("button", { name: "Sign out" }).click();
      await expect(other.getByLabel("Email address")).toBeVisible();
      expect(
        await other.evaluate(
          () =>
            Object.keys(localStorage).filter((key) =>
              key.startsWith("beweft.media-description:"),
            ).length,
        ),
      ).toBe(1);
      await other
        .getByLabel("Email address")
        .fill(process.env.BEWEFT_TEST_EMAIL!);
      await other
        .getByLabel("Password", { exact: true })
        .fill(process.env.BEWEFT_TEST_PASSWORD!);
      await other.getByRole("button", { name: "Sign in", exact: true }).click();
      await other
        .locator(".media-grid")
        .getByRole("button", { name: /unknown-session\.png/ })
        .click();
      await expect(
        other.getByLabel("Description", { exact: true }),
      ).toBeDisabled();
      await expect(
        other.getByRole("button", { name: "Retry this description" }),
      ).toBeVisible();
      await row.release();
      await expect
        .poll(async () => (await asset(context, item.id)).metadata_version)
        .toBe(2);
      await other.getByRole("button", { name: "Check description" }).click();
      await expect(
        other.getByLabel("Description", { exact: true }),
      ).toBeEnabled();
      await expect(
        other.getByLabel("Description", { exact: true }),
      ).toHaveValue("Unknown across sessions");
    } finally {
      await row.release();
      await other.close();
    }
  });
});

test("a late response for a resolved save cannot replace a newer same-dialog edit", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Late same-dialog response" });
    const item = await upload(context, brand.id, "same-dialog.png");
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    const held = deferred(),
      committed = deferred();
    let writes = 0;
    await page.route(`**/api/v1/media/${item.id}`, async (route) => {
      if (route.request().method() !== "PATCH") return route.continue();
      writes++;
      if (writes !== 1) return route.continue();
      const response = await route.fetch();
      committed.resolve();
      await held.promise;
      await route.fulfill({ response }).catch(() => {});
    });
    try {
      await page.goto("/#media");
      await page
        .locator(".media-grid")
        .getByRole("button", { name: /same-dialog\.png/ })
        .click();
      await page
        .getByLabel("Description", { exact: true })
        .fill("Original saved value");
      await page.getByRole("button", { name: "Save description" }).click();
      await committed.promise;
      await page
        .getByRole("button", { name: "Retry this description" })
        .click();
      await expect(
        page.getByLabel("Description", { exact: true }),
      ).toBeEnabled();
      await page
        .getByLabel("Description", { exact: true })
        .fill("New unsaved value");
      const delayed = page.waitForResponse(
        (response) =>
          response.request().method() === "PATCH" &&
          response.url().endsWith(`/api/v1/media/${item.id}`),
      );
      held.resolve();
      await (await delayed).finished();
      await page.evaluate(
        () =>
          new Promise<void>((resolve) =>
            requestAnimationFrame(() => resolve()),
          ),
      );
      await expect(page.getByLabel("Description", { exact: true })).toHaveValue(
        "New unsaved value",
      );
      expect(await asset(context, item.id)).toMatchObject({
        metadata_version: 2,
        alt_text: "Original saved value",
      });
      expect(writes).toBe(2);
    } finally {
      held.resolve();
    }
  });
});

test("a failed earlier response cannot leave a stale error after a successful recovery read", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", {
      name: "Overlapping media responses",
    });
    const item = await upload(context, brand.id, "overlap.png");
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    const committed = deferred(),
      failFirst = deferred(),
      checking = deferred(),
      finishCheck = deferred();
    let writes = 0;
    await page.route(`**/api/v1/media/${item.id}`, async (route) => {
      if (route.request().method() === "PATCH") {
        writes++;
        if (writes !== 1) return route.continue();
        await route.fetch();
        committed.resolve();
        await failFirst.promise;
        return route.abort("failed").catch(() => {});
      }
      if (route.request().method() === "GET" && writes >= 2) {
        const response = await route.fetch();
        checking.resolve();
        await finishCheck.promise;
        return route.fulfill({ response }).catch(() => {});
      }
      return route.continue();
    });
    try {
      await page.goto("/#media");
      await page
        .locator(".media-grid")
        .getByRole("button", { name: /overlap\.png/ })
        .click();
      await page.getByLabel("Description", { exact: true }).fill("Saved once");
      await page.getByRole("button", { name: "Save description" }).click();
      await committed.promise;
      await page
        .getByRole("button", { name: "Retry this description" })
        .click();
      await checking.promise;
      failFirst.resolve();
      await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
        "unconfirmed",
      );
      finishCheck.resolve();
      await expect(
        page.getByLabel("Description", { exact: true }),
      ).toBeEnabled();
      await expect(page.getByRole("dialog").getByRole("alert")).toHaveCount(0);
      expect(await asset(context, item.id)).toMatchObject({
        metadata_version: 2,
        alt_text: "Saved once",
      });
    } finally {
      failFirst.resolve();
      finishCheck.resolve();
    }
  });
});

test("unavailable intent storage blocks the write and lets the same edit recover", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Media storage failure" });
    const item = await upload(context, brand.id, "storage.png");
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.goto("/#media");
    await page
      .locator(".media-grid")
      .getByRole("button", { name: /storage\.png/ })
      .click();
    await page
      .getByLabel("Description", { exact: true })
      .fill("Keep this description");
    await page.evaluate(() => {
      const state = window as typeof window & {
        restoreMediaStorage: () => void;
      };
      const original = Storage.prototype.setItem;
      Storage.prototype.setItem = function (key, value) {
        if (key.startsWith("beweft.media-description:"))
          throw new Error("Media storage unavailable");
        return original.call(this, key, value);
      };
      state.restoreMediaStorage = () => {
        Storage.prototype.setItem = original;
      };
    });
    try {
      await page.getByRole("button", { name: "Save description" }).click();
      await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
        "Media storage unavailable",
      );
      expect(await asset(context, item.id)).toMatchObject({
        metadata_version: 1,
        alt_text: "",
      });
      await page.evaluate(() =>
        (
          window as typeof window & { restoreMediaStorage: () => void }
        ).restoreMediaStorage(),
      );
      await page.getByRole("button", { name: "Check description" }).click();
      await expect(
        page.getByLabel("Description", { exact: true }),
      ).toBeEnabled();
      await expect(page.getByLabel("Description", { exact: true })).toHaveValue(
        "Keep this description",
      );
      await page.getByRole("button", { name: "Save description" }).click();
      await expect(page.getByRole("dialog").getByRole("status")).toHaveText(
        "Description saved.",
      );
    } finally {
      await page
        .evaluate(() =>
          (
            window as typeof window & { restoreMediaStorage?: () => void }
          ).restoreMediaStorage?.(),
        )
        .catch(() => {});
    }
  });
});
