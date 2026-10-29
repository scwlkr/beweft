import { expect, test, type BrowserContext, type Page } from "@playwright/test";
import { deferred, withWorkspace } from "./helpers";

const expired = {
  error: { code: "unauthorized", message: "Old session expired" },
};

async function holdIdentityEvents(page: Page) {
  await page.addInitScript(() => {
    const state = window as typeof window & { holdAuthEpoch: boolean };
    state.holdAuthEpoch = true;
    window.addEventListener(
      "storage",
      (event) => {
        if (
          state.holdAuthEpoch &&
          (event.key === "beweft.auth-epoch" || event.key === null)
        )
          event.stopImmediatePropagation();
      },
      true,
    );
  });
}

async function releaseIdentityEvents(page: Page) {
  await page.evaluate(() => {
    (window as typeof window & { holdAuthEpoch: boolean }).holdAuthEpoch =
      false;
    window.dispatchEvent(
      new StorageEvent("storage", {
        key: "beweft.auth-epoch",
        newValue: localStorage.getItem("beweft.auth-epoch"),
        storageArea: localStorage,
      }),
    );
  });
}

async function signIn(page: Page) {
  await page.getByLabel("Email address").fill(process.env.BEWEFT_TEST_EMAIL!);
  await page
    .getByLabel("Password", { exact: true })
    .fill(process.env.BEWEFT_TEST_PASSWORD!);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
}

async function forceSignInView(page: Page) {
  await page.route("**/api/v1/providers", (route) =>
    route.fulfill({ status: 401, json: expired }),
  );
  await page.getByRole("button", { name: "Refresh workspace" }).click();
  await expect(page.getByLabel("Email address")).toBeVisible();
  await page.unroute("**/api/v1/providers");
}

async function fixture(
  page: Page,
  context: BrowserContext,
  write: (path: string, body: unknown) => Promise<any>,
) {
  const brand = await write("/brands", {
    name: `Cross-tab ${crypto.randomUUID()}`,
  });
  await write("/posts", {
    brand_id: brand.id,
    title: "Session fixture draft",
    text: "Replacement content",
    media_ids: [],
    variants: [],
  });
  await context.addInitScript(
    (id) => localStorage.setItem("beweft.brand", id),
    brand.id,
  );
  await page.goto("/#drafts");
  await expect(
    page.getByRole("heading", { name: "Session fixture draft" }),
  ).toBeVisible();
  const other = await context.newPage();
  other.setDefaultTimeout(10000);
  await other.goto("/#drafts");
  await expect(
    other.getByRole("heading", { name: "Session fixture draft" }),
  ).toBeVisible();
  return { brand, other };
}

test("a logout queued behind a login does not revoke the newer session", async () => {
  await withWorkspace(async (old, context, write) => {
    const { brand, other } = await fixture(old, context, write);
    const loginStarted = deferred(),
      releaseLogin = deferred();
    let logoutCalls = 0;
    await old.route("**/api/v1/auth/logout", (route) => {
      logoutCalls++;
      return route.continue();
    });
    try {
      await forceSignInView(other);
      await other.route("**/api/v1/auth/login", async (route) => {
        const response = await route.fetch();
        loginStarted.resolve();
        await releaseLogin.promise;
        await route.fulfill({ response });
      });
      await signIn(other);
      await loginStarted.promise;
      await old.getByRole("button", { name: "Sign out" }).click();
      await expect(
        old.getByRole("button", { name: "Signing out…" }),
      ).toBeDisabled();
      releaseLogin.resolve();
      await expect(other.getByLabel("Current brand")).toHaveValue(brand.id);
      await expect(
        old.getByRole("heading", { name: "Session fixture draft" }),
      ).toBeVisible();
      expect(logoutCalls).toBe(0);
      expect((await context.request.get("/api/v1/auth/me")).status()).toBe(200);
    } finally {
      releaseLogin.resolve();
      await other.close();
    }
  });
});

test("an old tab checks its represented session even when a login event is delayed", async () => {
  await withWorkspace(async (old, context, write) => {
    await holdIdentityEvents(old);
    const { brand, other } = await fixture(old, context, write);
    let logoutCalls = 0;
    await old.route("**/api/v1/auth/logout", (route) => {
      logoutCalls++;
      return route.continue();
    });
    try {
      await forceSignInView(other);
      await signIn(other);
      await expect(other.getByLabel("Current brand")).toHaveValue(brand.id);
      await old.getByRole("button", { name: "Sign out" }).click();
      await expect(
        old.getByRole("heading", { name: "Session fixture draft" }),
      ).toBeVisible();
      expect(logoutCalls).toBe(0);
      expect((await context.request.get("/api/v1/auth/me")).status()).toBe(200);
      await releaseIdentityEvents(old);
      await expect(old.getByLabel("Current brand")).toHaveValue(brand.id);
    } finally {
      await other.close();
    }
  });
});

test("a failed login does not announce a replacement session", async () => {
  await withWorkspace(async (old, context, write) => {
    const { brand, other } = await fixture(old, context, write);
    try {
      await forceSignInView(other);
      const before = await old.evaluate(() =>
        localStorage.getItem("beweft.auth-epoch"),
      );
      await other
        .getByLabel("Email address")
        .fill(process.env.BEWEFT_TEST_EMAIL!);
      await other
        .getByLabel("Password", { exact: true })
        .fill("incorrect password");
      await other.getByRole("button", { name: "Sign in", exact: true }).click();
      await expect(other.getByRole("alert")).toBeVisible();
      expect(
        await old.evaluate(() => localStorage.getItem("beweft.auth-epoch")),
      ).toBe(before);
      await expect(old.getByLabel("Current brand")).toHaveValue(brand.id);
      expect((await context.request.get("/api/v1/auth/me")).status()).toBe(200);
    } finally {
      await other.close();
    }
  });
});

for (const path of ["workspace", "direct", "composer", "action"] as const) {
  test(`a delayed ${path} 401 from an earlier cross-tab session cannot clear its replacement`, async () => {
    await withWorkspace(async (old, context, write) => {
      await holdIdentityEvents(old);
      const { brand, other } = await fixture(old, context, write);
      const requested = deferred(),
        release = deferred(),
        delivered = deferred();
      let intercepted = false;
      const pattern =
        path === "workspace"
          ? "**/api/v1/providers"
          : path === "direct"
            ? "**/api/v1/analytics?*"
            : "**/api/v1/posts/*";
      await old.route(pattern, async (route) => {
        const method = route.request().method();
        if (
          intercepted ||
          (path === "composer" && method !== "PATCH") ||
          (path === "action" && method !== "DELETE")
        )
          return route.continue();
        intercepted = true;
        requested.resolve();
        await release.promise;
        await route.fulfill({ status: 401, json: expired });
        delivered.resolve();
      });
      try {
        if (path === "workspace")
          await old.getByRole("button", { name: "Refresh workspace" }).click();
        if (path === "direct")
          await old
            .getByRole("button", { name: "Analytics", exact: true })
            .click();
        if (path === "composer") {
          await old.getByRole("button", { name: "Edit draft" }).click();
          await old
            .getByRole("button", { name: "Save draft", exact: true })
            .click();
        }
        if (path === "action") {
          await old
            .getByRole("button", { name: "Delete Session fixture draft" })
            .click();
          await old
            .getByRole("button", { name: "Delete draft", exact: true })
            .click();
        }
        await requested.promise;
        await other.getByRole("button", { name: "Sign out" }).click();
        await expect(
          other.getByRole("button", { name: "Sign in", exact: true }),
        ).toBeEnabled();
        await signIn(other);
        await expect(other.getByLabel("Current brand")).toHaveValue(brand.id);
        release.resolve();
        await delivered.promise;
        await expect(old.getByLabel("Email address")).toHaveCount(0);
        await expect(old.getByLabel("Current brand")).toHaveValue(brand.id);
        expect((await context.request.get("/api/v1/auth/me")).status()).toBe(
          200,
        );
        await old.unroute(pattern);
        await releaseIdentityEvents(old);
        await expect(old.getByRole("dialog")).toHaveCount(0);
        await old.getByRole("button", { name: /^Drafts/ }).click();
        await expect(
          old.getByRole("heading", { name: "Session fixture draft" }),
        ).toBeVisible();
        await old.getByRole("button", { name: "Edit draft" }).click();
        await expect(old.getByLabel("Post text", { exact: true })).toHaveValue(
          "Replacement content",
        );
      } finally {
        release.resolve();
        await other.close();
      }
    });
  });
}

test("a late failed session recheck cannot clear a newer successful cross-tab login", async () => {
  await withWorkspace(async (old, context, write) => {
    const { brand, other } = await fixture(old, context, write);
    const requested = deferred(),
      release = deferred(),
      delivered = deferred();
    let hold = true;
    await old.route("**/api/v1/auth/me", async (route) => {
      if (!hold) return route.continue();
      hold = false;
      const response = await route.fetch();
      requested.resolve();
      await release.promise;
      await route.fulfill({ response });
      delivered.resolve();
    });
    try {
      await other.getByRole("button", { name: "Sign out" }).click();
      await requested.promise;
      await expect(
        other.getByRole("button", { name: "Sign in", exact: true }),
      ).toBeEnabled();
      await signIn(other);
      await expect(old.getByLabel("Current brand")).toHaveValue(brand.id);
      release.resolve();
      await delivered.promise;
      await expect(
        old.getByRole("heading", { name: "Session fixture draft" }),
      ).toBeVisible();
      await expect(old.getByLabel("Email address")).toHaveCount(0);
    } finally {
      release.resolve();
      await other.close();
    }
  });
});

test("a delayed successful direct response cannot populate the replacement session", async () => {
  await withWorkspace(async (old, context, write) => {
    await holdIdentityEvents(old);
    const { brand, other } = await fixture(old, context, write);
    const requested = deferred(),
      release = deferred(),
      delivered = deferred();
    await old.route("**/api/v1/analytics?*", async (route) => {
      requested.resolve();
      await release.promise;
      await route.fulfill({
        json: {
          data: {
            availability: "partial",
            reason: "Old private analytics response",
            summary: { scheduled: 0, published: 0, attention: 0 },
            metrics: [],
          },
        },
      });
      delivered.resolve();
    });
    try {
      await old.getByRole("button", { name: "Analytics", exact: true }).click();
      await requested.promise;
      await other.getByRole("button", { name: "Sign out" }).click();
      await expect(
        other.getByRole("button", { name: "Sign in", exact: true }),
      ).toBeEnabled();
      await signIn(other);
      await expect(other.getByLabel("Current brand")).toHaveValue(brand.id);
      release.resolve();
      await delivered.promise;
      await expect(old.getByText("Old private analytics response")).toHaveCount(
        0,
      );
      await old.unroute("**/api/v1/analytics?*");
      await releaseIdentityEvents(old);
      await old.getByRole("button", { name: /^Drafts/ }).click();
      await expect(
        old.getByRole("heading", { name: "Session fixture draft" }),
      ).toBeVisible();
    } finally {
      release.resolve();
      await other.close();
    }
  });
});

test("a cross-tab identity transition clears the old editor before replacement loads", async () => {
  await withWorkspace(async (old, context, write) => {
    await holdIdentityEvents(old);
    const { other } = await fixture(old, context, write);
    const requested = deferred(),
      release = deferred();
    await old.getByRole("button", { name: "Edit draft" }).click();
    try {
      await other.getByRole("button", { name: "Sign out" }).click();
      await expect(
        other.getByRole("button", { name: "Sign in", exact: true }),
      ).toBeEnabled();
      await signIn(other);
      await expect(
        other.getByRole("heading", { name: "Session fixture draft" }),
      ).toBeVisible();
      await old.route("**/api/v1/brands", async (route) => {
        const response = await route.fetch();
        requested.resolve();
        await release.promise;
        await route.fulfill({ response });
      });
      await releaseIdentityEvents(old);
      await requested.promise;
      await expect(old.getByRole("dialog")).toHaveCount(0);
      await expect(
        old.getByText("Replacement content", { exact: true }),
      ).toHaveCount(0);
      release.resolve();
      await expect(
        old.getByRole("heading", { name: "Session fixture draft" }),
      ).toBeVisible();
      await old.getByRole("button", { name: "Edit draft" }).click();
      await expect(old.getByLabel("Post text", { exact: true })).toHaveValue(
        "Replacement content",
      );
    } finally {
      release.resolve();
      await other.close();
    }
  });
});

test("unavailable site storage blocks authentication with an actionable error", async () => {
  await withWorkspace(async (page) => {
    await page.addInitScript(() => {
      Storage.prototype.getItem = () => {
        throw new Error("blocked");
      };
      Storage.prototype.setItem = () => {
        throw new Error("blocked");
      };
    });
    await page.goto("/#drafts");
    await expect(
      page.getByText(/Browser storage is unavailable/),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Sign in", exact: true }),
    ).toHaveCount(0);
  });
});

test("losing site storage clears an open editor and reports how to recover", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Storage session" });
    await write("/posts", {
      brand_id: brand.id,
      title: "Storage private draft",
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
    await page.evaluate(() => {
      Storage.prototype.getItem = () => {
        throw new Error("blocked");
      };
      Storage.prototype.setItem = () => {
        throw new Error("blocked");
      };
    });
    await page.getByRole("button", { name: "Save draft", exact: true }).click();
    await expect(
      page.getByText(/Browser storage is unavailable/),
    ).toBeVisible();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await expect(
      page.getByText("Private content", { exact: true }),
    ).toHaveCount(0);
  });
});
