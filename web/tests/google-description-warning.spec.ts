import { expect, test, type BrowserContext } from "@playwright/test";
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

test("mobile shared composer warns for Google’s effective described photo", async () => {
  await withWorkspace(async (page, context, write) => {
    await page.setViewportSize({ width: 390, height: 844 });
    const brand = await write("/brands", {
      name: "Google description fixture",
    });
    const google = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Google location",
      timezone: "UTC",
    });
    const other = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Other channel",
      timezone: "UTC",
    });
    const described = await upload(context, brand.id, "described.png");
    const plain = await upload(context, brand.id, "plain.png");
    const description = "A blue fixture photo";
    const updated = await context.request.patch(
      `/api/v1/media/${described.id}`,
      {
        data: {
          alt_text: description,
          metadata_version: described.metadata_version,
        },
        headers: {
          "X-Beweft-Request": "1",
          Origin: process.env.BEWEFT_TEST_URL!,
        },
      },
    );
    expect(updated.ok(), await updated.text()).toBe(true);
    const post = await write("/posts", {
      brand_id: brand.id,
      title: "Shared photo fixture",
      text: "Fixture text",
      media_ids: [described.id],
      variants: [{ channel_id: google.id }, { channel_id: other.id }],
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.route("**/channels?*", (route) =>
      route.fulfill({
        json: { data: [{ ...google, provider: "google_business" }, other] },
      }),
    );
    await page.goto("/#drafts");
    await page.getByRole("button", { name: "Edit draft" }).click();
    const dialog = page.getByRole("dialog");
    const warning = dialog.getByText(
      "Photo descriptions stay in beweft and are not sent to Google.",
      { exact: false },
    );
    await expect(
      dialog.getByRole("tab", { name: "Shared version" }),
    ).toHaveAttribute("aria-selected", "true");
    await expect(dialog).toContainText(description);
    await expect(warning).toBeVisible();
    await expect(warning).toBeInViewport();
    await dialog.getByLabel("When should this go out?").selectOption("exact");
    await dialog
      .getByLabel(/Date and time ·/)
      .fill(new Date(Date.now() + 7 * 86400000).toISOString().slice(0, 16));
    await expect(
      dialog.getByRole("button", { name: "Schedule post" }),
    ).toBeEnabled();

    // An explicit Google override replaces the shared photo for that channel.
    await dialog.getByRole("tab", { name: "Google location" }).click();
    await expect(warning).toHaveCount(1);
    await dialog.getByRole("button", { name: "Add media" }).click();
    const picker = dialog.locator(".media-picker-grid");
    await picker.getByRole("button", { name: /described\.png/ }).click();
    await picker.getByRole("button", { name: /plain\.png/ }).click();
    await dialog.getByRole("tab", { name: "Shared version" }).click();
    await expect(dialog).toContainText(description);
    await expect(warning).toHaveCount(0);

    // A described Google override still warns when shared media is empty.
    await dialog.getByRole("tab", { name: "Google location" }).click();
    await picker.getByRole("button", { name: /plain\.png/ }).click();
    await picker.getByRole("button", { name: /described\.png/ }).click();
    await dialog.getByRole("tab", { name: "Shared version" }).click();
    await dialog.getByRole("button", { name: "Remove described.png" }).click();
    await expect(warning).toBeVisible();
    await expect(warning).toBeInViewport();
    await picker.getByRole("button", { name: /described\.png/ }).click();
    await dialog.getByRole("button", { name: "Google location" }).click();
    await expect(warning).toHaveCount(0);
    await dialog.getByRole("button", { name: "Google location" }).click();
    await expect(warning).toBeVisible();
    await dialog.getByLabel("When should this go out?").selectOption("draft");
    const saved = page.waitForResponse(
      (response) =>
        response.url().endsWith(`/api/v1/posts/${post.id}`) &&
        response.request().method() === "PATCH",
    );
    await dialog.getByRole("button", { name: "Save draft" }).click();
    expect((await saved).ok()).toBe(true);
    const storedPost = (
      await (await context.request.get(`/api/v1/posts/${post.id}`)).json()
    ).data;
    const storedMedia = (
      await (await context.request.get(`/api/v1/media/${described.id}`)).json()
    ).data;
    expect(storedPost.media_ids).toEqual([described.id]);
    expect(
      storedPost.variants.find((v: any) => v.channel_id === google.id)
        .media_ids,
    ).toEqual([described.id]);
    expect(
      storedPost.variants.find((v: any) => v.channel_id === other.id).media_ids,
    ).toBeUndefined();
    expect(storedMedia.alt_text).toBe(description);
  });
});
