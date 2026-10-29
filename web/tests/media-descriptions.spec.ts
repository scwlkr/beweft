import { expect, test, type BrowserContext, type Page } from "@playwright/test";
import { withWorkspace } from "./helpers";

const pixel = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aL1sAAAAASUVORK5CYII=",
  "base64",
);
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

async function expectSavedDescription(
  page: Page,
  context: BrowserContext,
  id: string,
  text: string,
  version: number,
) {
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("status")).toHaveText(
    /^(Description saved\.|The description changed\. Showing the current saved version\.)$/,
  );
  await expect(dialog.getByLabel("Description", { exact: true })).toHaveValue(
    text,
  );
  await expect(dialog.getByLabel("Description", { exact: true })).toBeEnabled();
  await expect(dialog.getByRole("alert")).toHaveCount(0);
  await expect
    .poll(async () => {
      const response = await context.request.get(`/api/v1/media/${id}`);
      if (!response.ok()) return null;
      const { alt_text, metadata_version } = (await response.json()).data;
      return { alt_text, metadata_version };
    })
    .toEqual({ alt_text: text, metadata_version: version });
}

test("media descriptions remain distinct and scheduled snapshots keep their saved versions", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Description fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Description channel",
      timezone: "UTC",
    });
    const alpha = await upload(context, brand.id, "alpha.png");
    const beta = await upload(context, brand.id, "beta.png");
    const post = await write("/posts", {
      brand_id: brand.id,
      title: "Described post",
      text: "Fixture post",
      media_ids: [alpha.id, beta.id],
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
        request.url().includes("/api/v1/media/")
      )
        patches++;
    });
    await page.goto("/#media");
    await page
      .locator(".media-grid")
      .getByRole("button", { name: /alpha\.png/ })
      .click();
    const description = page.getByLabel("Description", { exact: true });
    await expect(description).toHaveValue("");
    await description.fill("🌟".repeat(2001));
    await expect(
      page.getByText("Descriptions must be 2,000 characters or fewer."),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Save description" }),
    ).toBeDisabled();
    expect(patches).toBe(0);
    await description.fill("A bright star over a blue field.");
    await page.getByRole("button", { name: "Save description" }).click();
    await expectSavedDescription(
      page,
      context,
      alpha.id,
      "A bright star over a blue field.",
      alpha.metadata_version + 1,
    );
    await expect(
      page.getByRole("button", { name: "Save description" }),
    ).toBeDisabled();
    await expect(
      page
        .getByRole("dialog")
        .getByRole("img", { name: "A bright star over a blue field." }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Close dialog" }).click();
    await page
      .locator(".media-grid")
      .getByRole("button", { name: /beta\.png/ })
      .click();
    await page
      .getByLabel("Description", { exact: true })
      .fill("A small white pixel.");
    await page.getByRole("button", { name: "Save description" }).click();
    await expectSavedDescription(
      page,
      context,
      beta.id,
      "A small white pixel.",
      beta.metadata_version + 1,
    );
    await expect(
      page.getByRole("button", { name: "Save description" }),
    ).toBeDisabled();
    await page.getByRole("button", { name: "Close dialog" }).click();
    await page
      .locator(".media-grid")
      .getByRole("button", { name: /alpha\.png/ })
      .click();
    await expect(page.getByLabel("Description", { exact: true })).toHaveValue(
      "A bright star over a blue field.",
    );
    await page.getByRole("button", { name: "Close dialog" }).click();
    await page
      .getByRole("button", { name: /^Drafts/ })
      .first()
      .click();
    await page.getByRole("button", { name: "Edit draft", exact: true }).click();
    await expect(
      page.getByText("alpha.png: A bright star over a blue field."),
    ).toBeVisible();
    await expect(
      page.getByText("beta.png: A small white pixel."),
    ).toBeVisible();
    await expect(
      page
        .getByRole("complementary")
        .getByRole("img", { name: "A bright star over a blue field." }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Close dialog" }).click();
    const [publication] = await write(`/posts/${post.id}/schedule`, {
      version: post.version,
      channel_ids: [channel.id],
      scheduled_at: new Date(Date.now() + 3 * 86400_000).toISOString(),
    });
    const metadata = publication.payload.media_metadata as {
      id: string;
      alt_text: string;
    }[];
    expect(
      Object.fromEntries(metadata.map((item) => [item.id, item.alt_text])),
    ).toEqual({
      [alpha.id]: "A bright star over a blue field.",
      [beta.id]: "A small white pixel.",
    });
    await page
      .getByRole("button", { name: "Media library", exact: true })
      .click();
    await page
      .locator(".media-grid")
      .getByRole("button", { name: /alpha\.png/ })
      .click();
    await page
      .getByLabel("Description", { exact: true })
      .fill("A later edit to the description.");
    await page.getByRole("button", { name: "Save description" }).click();
    await expectSavedDescription(
      page,
      context,
      alpha.id,
      "A later edit to the description.",
      alpha.metadata_version + 2,
    );
    await expect(
      page.getByRole("button", { name: "Save description" }),
    ).toBeDisabled();
    const response = await context.request.get(
      `/api/v1/publications?id=${publication.id}&limit=1`,
    );
    expect(response.ok()).toBe(true);
    const saved = (await response.json()).data[0];
    expect(
      saved.payload.media_metadata.find(
        (item: { id: string }) => item.id === alpha.id,
      ).alt_text,
    ).toBe("A bright star over a blue field.");
    expect(patches).toBe(3);
  });
});

test("a completed description save cannot reopen another brand's media dialog", async () => {
  await withWorkspace(async (page, context, write) => {
    const alpha = await write("/brands", { name: "Description Alpha" });
    const beta = await write("/brands", { name: "Description Beta" });
    const alphaMedia = await upload(context, alpha.id, "alpha-switch.png");
    await upload(context, beta.id, "beta-switch.png");
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      alpha.id,
    );
    let release!: () => void, started!: () => void;
    const held = new Promise<void>((resolve) => (release = resolve));
    const saved = new Promise<void>((resolve) => (started = resolve));
    await page.route(`**/api/v1/media/${alphaMedia.id}`, async (route) => {
      if (route.request().method() !== "PATCH") return route.continue();
      const response = await route.fetch();
      started();
      await held;
      await route.fulfill({ response }).catch(() => {});
    });
    try {
      await page.goto("/#media");
      await page
        .locator(".media-grid")
        .getByRole("button", { name: /alpha-switch\.png/ })
        .click();
      await page
        .getByLabel("Description", { exact: true })
        .fill("Alpha's saved description");
      await page.getByRole("button", { name: "Save description" }).click();
      await saved;
      await page.getByRole("button", { name: "Close dialog" }).click();
      await page.getByLabel("Current brand").selectOption(beta.id);
      await expect(
        page
          .locator(".media-grid")
          .getByRole("button", { name: /beta-switch\.png/ }),
      ).toBeVisible();
      release();
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await expect(page.getByText("Description saved")).toHaveCount(0);
      await page.setViewportSize({ width: 390, height: 844 });
      await page
        .locator(".media-grid")
        .getByRole("button", { name: /beta-switch\.png/ })
        .click();
      await expect(
        page.getByLabel("Description", { exact: true }),
      ).toBeVisible();
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      ).toBe(true);
    } finally {
      release();
    }
  });
});

test("reopening the same media during a save shows the confirmed description", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Description reopen" });
    const asset = await upload(context, brand.id, "reopen.png");
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    let release!: () => void, started!: () => void;
    const held = new Promise<void>((resolve) => (release = resolve));
    const saved = new Promise<void>((resolve) => (started = resolve));
    await page.route(`**/api/v1/media/${asset.id}`, async (route) => {
      if (route.request().method() !== "PATCH") return route.continue();
      const response = await route.fetch();
      started();
      await held;
      await route.fulfill({ response }).catch(() => {});
    });
    try {
      await page.goto("/#media");
      await page
        .locator(".media-grid")
        .getByRole("button", { name: /reopen\.png/ })
        .click();
      await page
        .getByLabel("Description", { exact: true })
        .fill("A confirmed description");
      await page.getByRole("button", { name: "Save description" }).click();
      await saved;
      await page.getByRole("button", { name: "Close dialog" }).click();
      await page
        .locator(".media-grid")
        .getByRole("button", { name: /reopen\.png/ })
        .click();
      // The authoritative later version fences the response still held above.
      await expect(page.getByLabel("Description", { exact: true })).toHaveValue(
        "A confirmed description",
      );
      await expect(
        page.getByLabel("Description", { exact: true }),
      ).toBeEnabled();
      release();
      await expect(page.getByLabel("Description", { exact: true })).toHaveValue(
        "A confirmed description",
      );
      await expect(
        page.getByRole("button", { name: "Save description" }),
      ).toBeDisabled();
    } finally {
      release();
    }
  });
});
