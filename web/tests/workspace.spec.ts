import { test, expect, chromium } from "@playwright/test";

test("owner creates a brand, idea, manual channel, draft, schedule and media; mobile remains usable", async () => {
  if (
    process.env.BEWEFT_TEST_ISOLATED !== "1" ||
    !process.env.BEWEFT_TEST_URL ||
    !process.env.BEWEFT_TEST_EMAIL ||
    !process.env.BEWEFT_TEST_PASSWORD
  )
    throw new Error(
      "Use pnpm test:e2e to run against an isolated disposable server.",
    );
  const browser = await chromium.launch({ headless: true });
  try {
    const context = await browser.newContext({
      viewport: { width: 1440, height: 1000 },
    });
    const page = await context.newPage();
    const failures: string[] = [];
    page.on("pageerror", (error) => failures.push(error.message));
    await page.goto(process.env.BEWEFT_TEST_URL!);
    await page.getByLabel("Email address").fill(process.env.BEWEFT_TEST_EMAIL!);
    await page
      .getByLabel("Password", { exact: true })
      .fill(process.env.BEWEFT_TEST_PASSWORD!);
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(
      page.getByRole("heading", { name: "Your publishing rhythm" }),
    ).toBeVisible();
    await page
      .getByRole("button", { name: "Create brand", exact: true })
      .click();
    const brandName = `UI verification ${Date.now()}`;
    await page.getByLabel("Brand name").fill(brandName);
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Create brand", exact: true })
      .click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await expect(
      page.getByRole("combobox", { name: "Current brand" }),
    ).toHaveValue(/.+/);
    await page
      .getByRole("button", { name: "Connections", exact: true })
      .click();
    await page
      .getByRole("button", { name: "Add channel", exact: true })
      .click();
    await page.getByLabel("Channel name").fill("Manual QA channel");
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Add channel", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Manual QA channel", exact: true }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Settings", exact: true }).click();
    await page.getByRole("button", { name: "Add posting time" }).click();
    await expect(
      page.getByRole("button", { name: /Remove .* schedule/ }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Ideas", exact: true }).click();
    await page
      .getByRole("button", { name: "Capture an idea", exact: true })
      .click();
    await page.getByLabel("Idea title").fill("A very good spark");
    await page
      .getByLabel("Notes", { exact: true })
      .fill("A real local workflow, never a live social post.");
    await page.getByRole("button", { name: "Save idea", exact: true }).click();
    await expect(
      page.getByRole("heading", { name: "A very good spark", exact: true }),
    ).toBeVisible();
    await page
      .getByRole("button", { name: "Create post", exact: true })
      .click();
    await page.getByRole("button", { name: "Manual QA channel" }).click();
    await page.getByRole("button", { name: "Save draft", exact: true }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await page
      .getByRole("button", { name: /^Drafts/ })
      .first()
      .click();
    await page.getByRole("button", { name: "Edit draft", exact: true }).click();
    await page.getByLabel("When should this go out?").selectOption("queue");
    await page
      .getByRole("button", { name: "Add to queue", exact: true })
      .click();
    await expect(page.getByRole("dialog"))
      .toHaveCount(0)
      .catch(async () => {
        throw new Error(await page.getByRole("dialog").innerText());
      });
    await page
      .getByRole("button", { name: /^Publishing/ })
      .first()
      .click();
    await expect(
      page.getByRole("heading", { name: "A very good spark" }),
    ).toBeVisible();
    await page
      .getByRole("button", { name: "View details", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Attempt history" }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Close dialog" }).click();
    await page
      .getByRole("button", { name: "Media library", exact: true })
      .click();
    await expect(
      page.getByRole("button", { name: "Upload media", exact: true }),
    ).toBeEnabled();
    await page.getByLabel("Upload media files").setInputFiles({
      name: "qa-pixel.png",
      mimeType: "image/png",
      buffer: Buffer.from(
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aL1sAAAAASUVORK5CYII=",
        "base64",
      ),
    });
    await expect(
      page.getByRole("button", { name: /qa-pixel.png/ }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Analytics", exact: true }).click();
    await expect(
      page.getByText("Waiting for a supported connection"),
    ).toBeVisible();
    await page.getByRole("button", { name: "Settings", exact: true }).click();
    await page.getByRole("button", { name: "Appearance", exact: true }).click();
    await page.getByRole("button", { name: "Dark", exact: true }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await page.setViewportSize({ width: 390, height: 844 });
    await page.getByRole("button", { name: "Open navigation" }).click();
    await page.getByRole("button", { name: "Calendar", exact: true }).click();
    await expect(
      page.getByRole("heading", { name: "The bigger picture" }),
    ).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
    await page
      .getByRole("button", { name: "Create post", exact: true })
      .click();
    await expect(page.getByLabel("Post text", { exact: true })).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
    await page.getByRole("button", { name: "Close dialog" }).click();
    expect(failures).toEqual([]);
    // The isolated server test database is discarded by the integration harness.
    // No provider account or public queue is accessed by this test.
  } finally {
    await browser.close();
  }
});
