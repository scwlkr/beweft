import { expect, test } from "@playwright/test";
import { withWorkspace } from "./helpers";

test("manual channels select a destination and handoffs show current guidance without publishing", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Manual guidance fixture" });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.goto("/#connections");
    await page
      .getByRole("button", { name: "Add channel", exact: true })
      .click();
    await expect(
      page.getByLabel("Handoff destination", { exact: true }),
    ).toBeEnabled();
    await page
      .getByLabel("Handoff destination", { exact: true })
      .selectOption("youtube");
    await page.getByLabel("Channel name").fill("Manual video");
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Add channel", exact: true })
      .click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await expect(
      page.getByText(/Manual handoff · YouTube · Owner confirms publication/),
    ).toBeVisible();
    await page
      .getByLabel("Handoff destination for Manual video")
      .selectOption("instagram");
    await expect(
      page.getByText(/Manual handoff · Instagram · Owner confirms publication/),
    ).toBeVisible();
    const channels = (
      await (
        await context.request.get(`/api/v1/channels?brand_id=${brand.id}`)
      ).json()
    ).data;
    const channel = channels.find(
      (item: { name: string }) => item.name === "Manual video",
    );
    expect(channel.target_platform).toBe("instagram");
    const pixel = Buffer.from(
      "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aL1sAAAAASUVORK5CYII=",
      "base64",
    );
    const upload = await context.request.post(
      `/api/v1/media?brand_id=${brand.id}`,
      {
        headers: {
          "X-Beweft-Request": "1",
          Origin: process.env.BEWEFT_TEST_URL!,
        },
        multipart: {
          file: {
            name: "handoff-pixel.png",
            mimeType: "image/png",
            buffer: pixel,
          },
        },
      },
    );
    expect(upload.ok(), await upload.text()).toBe(true);
    const asset = (await upload.json()).data;
    const post = await write("/posts", {
      brand_id: brand.id,
      title: "Manual handoff",
      text: "A local-only fixture",
      media_ids: [asset.id],
      variants: [],
    });
    const [publication] = await write(`/posts/${post.id}/schedule`, {
      version: post.version,
      channel_ids: [channel.id],
      scheduled_at: new Date(Date.now() + 1000).toISOString(),
    });
    await expect
      .poll(
        async () => {
          const response = await context.request.get(
            `/api/v1/publications?id=${publication.id}&limit=1`,
          );
          return (await response.json()).data[0].state;
        },
        { timeout: 20_000 },
      )
      .toBe("manual_required");
    await page
      .getByRole("button", { name: /^Publishing/ })
      .first()
      .click();
    await page.getByRole("button", { name: "Refresh workspace" }).click();
    await page.getByRole("button", { name: "Complete handoff" }).click();
    await expect(
      page.getByRole("heading", { name: "Current destination · Instagram" }),
    ).toBeVisible();
    await expect(page.getByText(/Open Instagram's create flow/)).toBeVisible();
    const link = page.getByRole("link", { name: "Open Instagram" });
    await expect(link).toHaveAttribute("href", "https://www.instagram.com/");
    await expect(
      page.getByText(/This button does not publish for you/),
    ).toBeVisible();
    await context.grantPermissions(["clipboard-read", "clipboard-write"]);
    await page.getByRole("button", { name: "Copy post text" }).click();
    await expect(page.getByText("Caption copied")).toBeVisible();
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
      "A local-only fixture",
    );
    const downloadPromise = page.waitForEvent("download");
    await page
      .getByRole("link", { name: "Download handoff-pixel.png" })
      .click();
    const download = await downloadPromise;
    expect(download.suggestedFilename()).toBe("handoff-pixel.png");
    const stream = await download.createReadStream();
    const chunks: Buffer[] = [];
    for await (const chunk of stream) chunks.push(Buffer.from(chunk));
    expect(Buffer.concat(chunks)).toEqual(pixel);
    const readState = async () =>
      (
        await (
          await context.request.get(
            `/api/v1/publications?id=${publication.id}&limit=1`,
          )
        ).json()
      ).data[0].state;
    expect(await readState()).toBe("manual_required");
    await page
      .getByLabel("Completion note")
      .fill("Published manually in the isolated fixture.");
    await page.getByRole("button", { name: "I published this post" }).click();
    await expect.poll(readState).toBe("published");
  });
});

test("a destination outage keeps generic handoffs usable and permits an explicit retry", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Manual destination outage" });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    let unavailable = true;
    await page.route("**/api/v1/manual-platforms", (route) =>
      unavailable
        ? route.fulfill({
            status: 503,
            json: {
              error: {
                code: "unavailable",
                message: "Destination list unavailable",
              },
            },
          })
        : route.continue(),
    );
    await page.goto("/#connections");
    await expect(
      page.getByText(/Manual destinations are unavailable/),
    ).toBeVisible();
    await page
      .getByRole("button", { name: "Add channel", exact: true })
      .click();
    await expect(
      page.getByLabel("Handoff destination", { exact: true }),
    ).toBeDisabled();
    await page.getByLabel("Channel name").fill("Generic handoff");
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Add channel", exact: true })
      .click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    const channels = (
      await (
        await context.request.get(`/api/v1/channels?brand_id=${brand.id}`)
      ).json()
    ).data;
    expect(
      channels.find((item: { name: string }) => item.name === "Generic handoff")
        .target_platform,
    ).toBe("generic");
    unavailable = false;
    await page.getByRole("button", { name: "Retry destinations" }).click();
    await expect(
      page.getByLabel("Handoff destination for Generic handoff"),
    ).toBeEnabled();
    await page
      .getByRole("button", { name: "Add channel", exact: true })
      .click();
    await page.getByLabel("Channel type").selectOption("fixture");
    await expect(
      page.getByLabel("Handoff destination", { exact: true }),
    ).toHaveCount(0);
    await page.getByLabel("Channel name").fill("Isolated fixture channel");
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Add channel", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Isolated fixture channel" }),
    ).toBeVisible();
  });
});

test("a non-HTTPS manual destination is never rendered as a link", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Manual URL fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Manual target",
      target_platform: "x",
      timezone: "UTC",
    });
    const post = await write("/posts", {
      brand_id: brand.id,
      title: "Unsafe link fixture",
      text: "Fixture",
      media_ids: [],
      variants: [],
    });
    const [publication] = await write(`/posts/${post.id}/schedule`, {
      version: post.version,
      channel_ids: [channel.id],
      scheduled_at: new Date(Date.now() + 86400_000).toISOString(),
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.route("**/channels?*", async (route) => {
      const response = await route.fetch(),
        body = await response.json();
      body.data[0].manual_guidance.compose_url =
        "http://example.invalid/compose";
      await route.fulfill({ response, json: body });
    });
    await page.route("**/publications?*", (route) =>
      route.fulfill({
        json: { data: [{ ...publication, state: "manual_required" }] },
      }),
    );
    await page.goto("/#queue");
    await page.getByRole("button", { name: "Complete handoff" }).click();
    await expect(
      page.getByRole("heading", { name: "Current destination · X" }),
    ).toBeVisible();
    await expect(page.getByRole("link", { name: "Open X" })).toHaveCount(0);
  });
});
