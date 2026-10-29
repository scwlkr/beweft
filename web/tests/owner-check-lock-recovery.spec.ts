import { createServer, request, type ServerResponse } from "node:http";
import { fileURLToPath } from "node:url";
import { expect, test, type BrowserContext, type Page } from "@playwright/test";
import { build } from "vite";
import { withWorkspace } from "./helpers";

async function withStalledOwner(
  page: Page,
  context: BrowserContext,
  write: (path: string, body: unknown) => Promise<any>,
  run: (fixture: {
    pages: Page[];
    held: Set<ServerResponse>;
    starts: () => number;
    logouts: () => number;
    sends: () => number;
    stall: (value: boolean) => void;
  }) => Promise<void>,
) {
  const brand = await write("/brands", { name: "Owner lock fixture" });
  const itemId = crypto.randomUUID();
  await context.addInitScript(
    (id) => localStorage.setItem("beweft.brand", id),
    brand.id,
  );
  await context.route("**/inbox?*", (route) =>
    route.fulfill({
      json: {
        data: {
          availability: "partial",
          items: [
            {
              id: itemId,
              channel_id: crypto.randomUUID(),
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
  await context.route(`**/inbox/${itemId}/replies`, (route) =>
    route.fulfill({ json: { data: [] } }),
  );
  let sends = 0;
  await context.route(`**/inbox/${itemId}/reply`, (route) => {
    sends++;
    return route.abort("failed");
  });
  const upstream = new URL(process.env.BEWEFT_TEST_URL!);
  const held = new Set<ServerResponse>();
  let stall = false,
    starts = 0,
    logouts = 0;
  const proxy = createServer((incoming, outgoing) => {
    const path = incoming.url ?? "/";
    if (path === "/pending-fixture") {
      outgoing.writeHead(200, { "Content-Type": "text/html" });
      outgoing.end(
        '<!doctype html><html><body><div id="root"></div></body></html>',
      );
      return;
    }
    if (stall && path === "/api/v1/auth/me") {
      starts++;
      held.add(outgoing);
      outgoing.on("close", () => held.delete(outgoing));
      outgoing.writeHead(200, {
        "Content-Type": "application/json",
        "Cache-Control": "no-store",
      });
      outgoing.flushHeaders();
      outgoing.write('{"data":'); // Real HTTP/1 headers and partial body; do not finish it.
      return;
    }
    if (path === "/api/v1/auth/logout") logouts++;
    const headers = { ...incoming.headers, host: upstream.host };
    if (headers.origin) headers.origin = upstream.origin;
    const forward = request(
      new URL(path, upstream),
      { method: incoming.method, headers },
      (response) => {
        outgoing.writeHead(response.statusCode ?? 502, response.headers);
        response.pipe(outgoing);
      },
    );
    forward.on("error", () => {
      if (!outgoing.destroyed) {
        if (!outgoing.headersSent) outgoing.writeHead(502);
        outgoing.end();
      }
    });
    incoming.on("aborted", () => forward.destroy());
    outgoing.on("close", () => {
      if (!outgoing.writableEnded) forward.destroy();
    });
    incoming.pipe(forward);
  });
  await new Promise<void>((resolve) => proxy.listen(0, "127.0.0.1", resolve));
  const address = proxy.address();
  if (!address || typeof address === "string")
    throw new Error("Proxy did not bind to loopback");
  const pages = [page, await context.newPage(), await context.newPage()];
  try {
    for (const tab of pages) {
      await tab.goto(`http://127.0.0.1:${address.port}/#inbox`);
      await expect(
        tab.getByRole("button", { name: "Reply", exact: true }),
      ).toBeVisible();
    }
    await run({
      pages,
      held,
      starts: () => starts,
      logouts: () => logouts,
      sends: () => sends,
      stall: (value) => (stall = value),
    });
  } finally {
    for (const tab of pages) {
      if (!tab.isClosed()) await tab.goto("about:blank").catch(() => {});
    }
    proxy.closeAllConnections();
    await new Promise<void>((resolve, reject) =>
      proxy.close((error) => (error ? reject(error) : resolve())),
    );
  }
}

async function pendingAuthLocks(page: Page) {
  return page.evaluate(
    async () =>
      (await navigator.locks.query()).pending.filter(
        (lock) => lock.name === "beweft:authentication",
      ).length,
  );
}

async function pendingFixtureScript() {
  const built = await build({
    configFile: false,
    logLevel: "silent",
    define: { "process.env.NODE_ENV": '"production"' },
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL("./fixtures/pending-schedule-owner.tsx", import.meta.url),
        ),
        name: "PendingScheduleFixture",
        formats: ["iife"],
      },
    },
  });
  const bundle = Array.isArray(built) ? built[0] : built;
  const script = bundle.output.find((item) => item.type === "chunk");
  if (!script || script.type !== "chunk")
    throw new Error("Fixture bundle missing");
  return script.code;
}

test("two stalled reply owner checks cannot starve a third tab's sign-out", async () => {
  await withWorkspace(async (page, context, write) => {
    await withStalledOwner(page, context, write, async (fixture) => {
      const [first, second, signingOut] = fixture.pages;
      fixture.stall(true);
      await first.getByRole("button", { name: "Reply", exact: true }).click();
      await expect.poll(fixture.starts).toBe(1);
      await second.getByRole("button", { name: "Reply", exact: true }).click();
      await expect
        .poll(() => pendingAuthLocks(second))
        .toBeGreaterThanOrEqual(1);
      await expect(first.getByLabel("Your reply")).toBeDisabled();
      await expect(second.getByLabel("Your reply")).toBeDisabled();
      const logoutResponse = signingOut.waitForResponse(
        (response) => response.url().endsWith("/api/v1/auth/logout"),
        { timeout: 9_000 },
      );
      await signingOut.getByRole("button", { name: "Sign out" }).click();
      await expect
        .poll(() => pendingAuthLocks(first))
        .toBeGreaterThanOrEqual(2);
      await expect.poll(fixture.logouts, { timeout: 9_000 }).toBe(1);
      expect((await logoutResponse).status()).toBe(200);
      await expect.poll(() => fixture.held.size).toBe(0);
      expect((await context.request.get("/api/v1/auth/me")).status()).toBe(401);
      expect(fixture.sends()).toBe(0);

      fixture.stall(false);
      await signingOut.getByLabel("Email").fill(process.env.BEWEFT_TEST_EMAIL!);
      await signingOut
        .getByLabel("Password")
        .fill(process.env.BEWEFT_TEST_PASSWORD!);
      await signingOut
        .getByRole("button", { name: "Sign in", exact: true })
        .click();
      await expect(signingOut.getByLabel("Current brand")).toBeVisible({
        timeout: 10_000,
      });
      for (const tab of [first, second]) {
        await expect(
          tab.getByRole("button", { name: "Reply", exact: true }),
        ).toBeVisible({ timeout: 10_000 });
        await tab.getByRole("button", { name: "Reply", exact: true }).click();
        await expect(tab.getByLabel("Your reply")).toBeEnabled();
      }
      expect(fixture.sends()).toBe(0);
    });
  });
});

test("pending-schedule owner checks cancel on unmount and brand change", async () => {
  const script = await pendingFixtureScript();
  await withWorkspace(async (page, context, write) => {
    await withStalledOwner(page, context, write, async (fixture) => {
      const [first, pending] = fixture.pages;
      fixture.stall(true);
      await first.getByRole("button", { name: "Reply", exact: true }).click();
      await expect.poll(fixture.starts).toBe(1);
      await pending.goto(new URL("/pending-fixture", pending.url()).href);
      await pending.addScriptTag({ content: script });
      await expect(pending.getByText("Checking: first")).toBeVisible();
      await expect.poll(() => pendingAuthLocks(first)).toBe(1);
      await pending.getByRole("button", { name: "Unmount" }).click();
      await expect
        .poll(() => pendingAuthLocks(first), { timeout: 2_000 })
        .toBe(0);
      await pending.getByRole("button", { name: "Mount" }).click();
      await expect.poll(() => pendingAuthLocks(first)).toBe(1);
      await pending.getByRole("button", { name: "Change brand" }).click();
      await expect(pending.getByText("Checking: second")).toBeVisible();
      await expect.poll(() => pendingAuthLocks(first)).toBe(1);
      fixture.stall(false);
      await first.evaluate(() => (location.hash = "#drafts"));
      await expect(pending.getByText("Ready: second")).toBeVisible();
      await expect.poll(() => fixture.held.size).toBe(0);
      await pending.evaluate(() => {
        (
          window as typeof window & { pendingFixtureStates: unknown[] }
        ).pendingFixtureStates = [];
      });
      fixture.stall(true);
      await pending.getByRole("button", { name: "Change brand" }).click();
      await expect(pending.getByText("Checking: first")).toBeVisible();
      await expect.poll(() => fixture.held.size).toBe(1);
      const states = await pending.evaluate(
        () =>
          (
            window as typeof window & {
              pendingFixtureStates: { brandId: string; ready: boolean }[];
            }
          ).pendingFixtureStates,
      );
      expect(
        states.some((state) => state.brandId === "first" && state.ready),
      ).toBe(false);
      await pending.getByRole("button", { name: "Unmount" }).click();
      await expect.poll(() => fixture.held.size, { timeout: 2_000 }).toBe(0);
      expect(fixture.sends()).toBe(0);
    });
  });
});

test("a failed owner check blocks reply until explicit recovery succeeds", async () => {
  await withWorkspace(async (page, context, write) => {
    await withStalledOwner(page, context, write, async (fixture) => {
      const [first] = fixture.pages;
      fixture.stall(true);
      await first.getByRole("button", { name: "Reply", exact: true }).click();
      await expect.poll(fixture.starts).toBe(1);
      await expect(first.getByText(/session check timed out/i)).toBeVisible({
        timeout: 8_000,
      });
      await expect(first.getByLabel("Your reply")).toBeDisabled();
      await expect(
        first.getByRole("button", { name: "Send reply" }),
      ).toBeDisabled();
      expect(fixture.sends()).toBe(0);
      fixture.stall(false);
      await first
        .getByRole("button", { name: "Check existing reply outcome" })
        .click();
      await expect(first.getByLabel("Your reply")).toBeEnabled();
      expect(fixture.sends()).toBe(0);
    });
  });
});

test("abandoned reply and workspace owner checks leave the lock queue promptly", async () => {
  await withWorkspace(async (page, context, write) => {
    await withStalledOwner(page, context, write, async (fixture) => {
      const [first, second, verifying] = fixture.pages;
      fixture.stall(true);
      await first.getByRole("button", { name: "Reply", exact: true }).click();
      await expect.poll(fixture.starts).toBe(1);
      await second.getByRole("button", { name: "Reply", exact: true }).click();
      await expect.poll(() => pendingAuthLocks(second)).toBe(1);
      await second.evaluate(() => (location.hash = "#drafts"));
      await expect
        .poll(() => pendingAuthLocks(first), { timeout: 2_000 })
        .toBe(0);
      await first.evaluate(() => (location.hash = "#drafts"));
      await expect.poll(() => fixture.held.size, { timeout: 2_000 }).toBe(0);

      await first.evaluate(() => (location.hash = "#inbox"));
      await first.getByRole("button", { name: "Reply", exact: true }).click();
      await expect.poll(fixture.starts).toBe(2);
      await verifying.reload({ waitUntil: "domcontentloaded" });
      await expect.poll(() => pendingAuthLocks(first)).toBe(1);
      await verifying.goto("about:blank");
      await expect
        .poll(() => pendingAuthLocks(first), { timeout: 2_000 })
        .toBe(0);
      await first.evaluate(() => (location.hash = "#drafts"));
      await expect.poll(() => fixture.held.size, { timeout: 2_000 }).toBe(0);

      fixture.stall(false);
      await first.evaluate(() => (location.hash = "#inbox"));
      await first.getByRole("button", { name: "Reply", exact: true }).click();
      await expect(first.getByLabel("Your reply")).toBeEnabled();
      expect(fixture.sends()).toBe(0);
    });
  });
});

test("a queued schedule claim rejects a superseded session before writing", async () => {
  const script = await pendingFixtureScript();
  await withWorkspace(async (page, context, write) => {
    await withStalledOwner(page, context, write, async (fixture) => {
      const [signingOut, pending, lockHolder] = fixture.pages;
      const key = "beweft.pending-schedule:fixture:brand";
      await pending.goto(new URL("/pending-fixture", pending.url()).href);
      await pending.addScriptTag({ content: script });
      await expect(pending.getByText("Ready: first")).toBeVisible();
      await lockHolder.evaluate((lockKey) => {
        const state = window as any;
        let release!: () => void, held!: () => void;
        const wait = new Promise<void>((resolve) => (release = resolve));
        state.scheduleHeld = new Promise<void>((resolve) => (held = resolve));
        state.releaseSchedule = release;
        void navigator.locks.request(lockKey, async () => {
          held();
          await wait;
        });
      }, key);
      try {
        await lockHolder.evaluate(() => (window as any).scheduleHeld);
        await pending.evaluate((lockKey) => {
          const state = window as any,
            api = state.scheduleFixture;
          const now = new Date().toISOString();
          const post = {
            id: crypto.randomUUID(),
            brand_id: crypto.randomUUID(),
            title: "Fixture",
            text: "Original",
            media_ids: [],
            variants: [],
            version: 1,
            status: "draft",
            created_at: now,
            updated_at: now,
          };
          const intent = api.makeScheduleIntent(
            post,
            crypto.randomUUID(),
            [crypto.randomUUID()],
            "queue",
            "",
            {},
          );
          state.scheduleOutcome = api
            .scheduleLock(lockKey, (session: unknown) =>
              api.sendScheduleIntent(lockKey, intent, session),
            )
            .then(
              () => "saved",
              (error: { code?: string }) => error.code ?? "failed",
            );
        }, key);
        await expect
          .poll(() =>
            pending.evaluate(
              async (lockKey) =>
                (await navigator.locks.query()).pending.some(
                  (lock) => lock.name === lockKey,
                ),
              key,
            ),
          )
          .toBe(true);
        const logout = signingOut.waitForResponse((response) =>
          response.url().endsWith("/api/v1/auth/logout"),
        );
        await signingOut.getByRole("button", { name: "Sign out" }).click();
        expect((await logout).status()).toBe(200);
        expect((await context.request.get("/api/v1/auth/me")).status()).toBe(
          401,
        );
        await lockHolder.evaluate(() => (window as any).releaseSchedule());
        expect(
          await pending.evaluate(() => (window as any).scheduleOutcome),
        ).toBe("session_changed");
        expect(
          await pending.evaluate(
            (lockKey) => localStorage.getItem(lockKey),
            key,
          ),
        ).toBeNull();
        await expect(
          pending.getByText(/browser session changed/i),
        ).toBeVisible();
        await signingOut
          .getByLabel("Email")
          .fill(process.env.BEWEFT_TEST_EMAIL!);
        await signingOut
          .getByLabel("Password")
          .fill(process.env.BEWEFT_TEST_PASSWORD!);
        await signingOut
          .getByRole("button", { name: "Sign in", exact: true })
          .click();
        await expect(signingOut.getByLabel("Current brand")).toBeVisible();
        await pending.reload();
        await pending.addScriptTag({ content: script });
        await expect(pending.getByText("Ready: first")).toBeVisible();
      } finally {
        await lockHolder
          .evaluate(() => (window as any).releaseSchedule?.())
          .catch(() => {});
      }
    });
  });
});

test("a schedule with an unknown response keeps its intent across sign-out", async () => {
  const script = await pendingFixtureScript();
  await withWorkspace(async (page, context, write) => {
    await withStalledOwner(page, context, write, async (fixture) => {
      const [signingOut, pending] = fixture.pages;
      const key = "beweft.pending-schedule:fixture:unknown";
      let started!: () => void,
        release!: () => void,
        sends = 0;
      const posted = new Promise<void>((resolve) => (started = resolve));
      const held = new Promise<void>((resolve) => (release = resolve));
      await context.route(
        /\/api\/v1\/posts\/[^/]+\/schedule\?view=summary$/,
        async (route) => {
          sends++;
          started();
          await held;
          await route.abort("failed").catch(() => {});
        },
      );
      try {
        await pending.goto(new URL("/pending-fixture", pending.url()).href);
        await pending.addScriptTag({ content: script });
        await expect(pending.getByText("Ready: first")).toBeVisible();
        await pending.evaluate((lockKey) => {
          const state = window as any,
            api = state.scheduleFixture;
          const now = new Date().toISOString();
          const post = {
            id: crypto.randomUUID(),
            brand_id: crypto.randomUUID(),
            title: "Fixture",
            text: "Original",
            media_ids: [],
            variants: [],
            version: 1,
            status: "draft",
            created_at: now,
            updated_at: now,
          };
          state.intent = api.makeScheduleIntent(
            post,
            crypto.randomUUID(),
            [crypto.randomUUID()],
            "queue",
            "",
            {},
          );
          state.session = api.captureWorkspaceSession();
          state.scheduleOutcome = api
            .sendScheduleIntent(lockKey, state.intent, state.session)
            .then(
              () => "saved",
              (error: { code?: string }) => error.code ?? "failed",
            );
        }, key);
        await posted;
        const original = await pending.evaluate(
          (lockKey) => localStorage.getItem(lockKey),
          key,
        );
        expect(JSON.parse(original!).uncertain).toBe(true);
        const logout = signingOut.waitForResponse((response) =>
          response.url().endsWith("/api/v1/auth/logout"),
        );
        await signingOut.getByRole("button", { name: "Sign out" }).click();
        expect((await logout).status()).toBe(200);
        release();
        await pending.evaluate(() => (window as any).scheduleOutcome);
        expect(
          await pending.evaluate(
            (lockKey) => localStorage.getItem(lockKey),
            key,
          ),
        ).toBe(original);
        const clearOutcome = await pending.evaluate(async (lockKey) => {
          const state = window as any;
          try {
            await state.scheduleFixture.clearScheduleIntent(
              lockKey,
              state.intent,
              { kind: "rejected" },
              state.session,
            );
            return "cleared";
          } catch (error) {
            return (error as { code?: string }).code ?? "failed";
          }
        }, key);
        expect(clearOutcome).toBe("session_changed");
        expect(
          await pending.evaluate(
            (lockKey) => localStorage.getItem(lockKey),
            key,
          ),
        ).toBe(original);
        expect(sends).toBe(1);
      } finally {
        release();
      }
    });
  });
});
