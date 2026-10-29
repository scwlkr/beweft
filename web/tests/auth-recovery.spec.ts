import { expect, test } from "@playwright/test";
import { deferred, withWorkspace } from "./helpers";

const expired = { error: { code: "unauthorized", message: "Session expired" } };

async function signIn(page: import("@playwright/test").Page) {
  await page.getByLabel("Email address").fill(process.env.BEWEFT_TEST_EMAIL!);
  await page
    .getByLabel("Password", { exact: true })
    .fill(process.env.BEWEFT_TEST_PASSWORD!);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
}

test("a composer 401 clears private state and the stale editor", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Composer session" });
    await write("/posts", {
      brand_id: brand.id,
      title: "Private draft",
      text: "Private content",
      media_ids: [],
      variants: [],
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.goto("/#drafts");
    await page.getByRole("button", { name: "Edit draft" }).click();
    await page.route("**/api/v1/posts/*", (route) =>
      route.request().method() === "PATCH"
        ? route.fulfill({ status: 401, json: expired })
        : route.continue(),
    );
    await page.getByRole("button", { name: "Save draft", exact: true }).click();
    await expect(page.getByLabel("Email address")).toBeVisible();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await expect(
      page.getByText("Private content", { exact: true }),
    ).toHaveCount(0);
    await signIn(page);
    await expect(
      page.getByRole("heading", { name: "Private draft" }),
    ).toBeVisible();
    await expect(page.getByRole("dialog")).toHaveCount(0);
  });
});

test("a direct view 401 clears the workspace", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "View session" });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.route("**/api/v1/analytics?*", (route) =>
      route.fulfill({ status: 401, json: expired }),
    );
    await page.goto("/#analytics");
    await expect(page.getByLabel("Email address")).toBeVisible();
    await expect(page.getByText("View session", { exact: true })).toHaveCount(
      0,
    );
  });
});

test("a delayed composer 401 cannot sign out a later session", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Delayed composer session" });
    await write("/posts", {
      brand_id: brand.id,
      title: "Delayed private draft",
      text: "Private",
      media_ids: [],
      variants: [],
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.clock.install();
    await page.goto("/#drafts");
    await page.getByRole("button", { name: "Edit draft" }).click();
    const oldRequested = deferred(),
      releaseOld = deferred(),
      delivered = deferred();
    await page.route("**/api/v1/posts/*", async (route) => {
      if (route.request().method() !== "PATCH") return route.continue();
      oldRequested.resolve();
      await releaseOld.promise;
      await route.fulfill({ status: 401, json: expired });
      delivered.resolve();
    });
    let expire = true;
    await page.route("**/api/v1/jobs/health", (route) => {
      if (!expire) return route.continue();
      expire = false;
      return route.fulfill({ status: 401, json: expired });
    });
    try {
      await page
        .getByRole("button", { name: "Save draft", exact: true })
        .click();
      await oldRequested.promise;
      await page.clock.fastForward(30_000);
      await expect(page.getByLabel("Email address")).toBeVisible();
      await signIn(page);
      await expect(
        page.getByRole("heading", { name: "Delayed private draft" }),
      ).toBeVisible();
      releaseOld.resolve();
      await delivered.promise;
      await expect(
        page.getByRole("heading", { name: "Delayed private draft" }),
      ).toBeVisible();
      await expect(page.getByLabel("Email address")).toHaveCount(0);
    } finally {
      releaseOld.resolve();
    }
  });
});

test("sign-in waits for a delayed logout response before starting a new session", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Logout race" });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.goto("/#drafts");
    await expect(page.getByLabel("Current brand")).toHaveValue(brand.id);
    const logoutStarted = deferred(),
      releaseLogout = deferred();
    await page.route("**/api/v1/auth/logout", async (route) => {
      const response = await route.fetch();
      logoutStarted.resolve();
      await releaseLogout.promise;
      await route.fulfill({ response });
    });
    try {
      await page.getByRole("button", { name: "Sign out" }).click();
      await logoutStarted.promise;
      await expect(page.getByLabel("Email address")).toBeVisible();
      await page
        .getByLabel("Email address")
        .fill(process.env.BEWEFT_TEST_EMAIL!);
      await page
        .getByLabel("Password", { exact: true })
        .fill(process.env.BEWEFT_TEST_PASSWORD!);
      await expect(
        page.getByRole("button", { name: "Signing out…" }),
      ).toBeDisabled();
      releaseLogout.resolve();
      await page.getByRole("button", { name: "Sign in", exact: true }).click();
      await expect(page.getByLabel("Current brand")).toHaveValue(brand.id);
      await page.reload();
      await expect(page.getByLabel("Current brand")).toHaveValue(brand.id);
    } finally {
      releaseLogout.resolve();
    }
  });
});

test("another tab cannot sign in before an earlier logout response completes", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Cross-tab logout" });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.goto("/#drafts");
    await expect(page.getByLabel("Current brand")).toHaveValue(brand.id);
    const logoutStarted = deferred(),
      releaseLogout = deferred();
    await page.route("**/api/v1/auth/logout", async (route) => {
      const response = await route.fetch();
      logoutStarted.resolve();
      await releaseLogout.promise;
      await route.fulfill({ response });
    });
    const other = await context.newPage();
    let logins = 0,
      sessionChecks = 0;
    await other.route("**/api/v1/auth/login", (route) => {
      logins++;
      return route.continue();
    });
    await other.route("**/api/v1/auth/me", (route) => {
      sessionChecks++;
      return route.continue();
    });
    try {
      await page.getByRole("button", { name: "Sign out" }).click();
      await logoutStarted.promise;
      await other.goto("/#drafts");
      await expect(other.getByText("Loading your workspace…")).toBeVisible();
      expect(sessionChecks).toBe(0);
      expect(logins).toBe(0);
      releaseLogout.resolve();
      await expect(other.getByLabel("Email address")).toBeVisible();
      await signIn(other);
      await expect(other.getByLabel("Current brand")).toHaveValue(brand.id);
      expect(logins).toBe(1);
      await other.reload();
      await expect(other.getByLabel("Current brand")).toHaveValue(brand.id);
    } finally {
      releaseLogout.resolve();
      await other.close();
    }
  });
});
