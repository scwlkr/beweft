import { execFileSync } from "node:child_process";
import { expect, test } from "@playwright/test";
import { withWorkspace } from "./helpers";

test.skip(
  process.env.BEWEFT_TEST_FORMAT_FIXTURE !== "1",
  "Requires the sandboxed, no-worker provider-format fixture harness",
);

function setFixtureProvider(channelId: string) {
  const db = process.env.BEWEFT_TEST_DB_NAME ?? "";
  if (
    !/^beweft_e2e_[a-f0-9]{12}$/.test(db) ||
    !/^[a-f0-9-]{36}$/.test(channelId)
  )
    throw new Error("Requires an isolated fixture database and channel");
  execFileSync(
    "docker",
    [
      "exec",
      process.env.BEWEFT_TEST_POSTGRES_CONTAINER ?? "beweft-dev-postgres",
      "psql",
      "-U",
      process.env.BEWEFT_TEST_DB_USER ?? "beweft",
      "-d",
      db,
      "-v",
      "ON_ERROR_STOP=1",
      "-c",
      `UPDATE channels SET provider='x', target_platform=NULL, paused=true, connection_status='connected' WHERE id='${channelId}'`,
    ],
    { stdio: "ignore" },
  );
}

test("a lost local format rejection recovers its receipt before correction", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", {
      name: `Format fixture ${crypto.randomUUID()}`,
    });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Isolated format channel",
      timezone: "UTC",
    });
    const post = await write("/posts", {
      brand_id: brand.id,
      title: "Format draft",
      text: "x".repeat(281),
      media_ids: [],
      variants: [{ channel_id: channel.id }],
    });
    setFixtureProvider(channel.id);
    const ownerResponse = await context.request.get("/api/v1/auth/me");
    const owner = (await ownerResponse.json()).data;
    const key = `beweft.pending-schedule:${owner.id}:${brand.id}`;
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    const requests: any[] = [];
    await page.route(
      new RegExp(`/api/v1/posts/${post.id}/schedule\\?view=summary$`),
      async (route) => {
        requests.push(route.request().postDataJSON());
        if (requests.length !== 1) return route.continue();
        const response = await route.fetch();
        expect(response.status()).toBe(400);
        expect((await response.json()).error.code).toBe(
          "provider_format_invalid",
        );
        await route.abort("failed");
      },
    );
    await page.goto("/#drafts");
    await page.getByRole("button", { name: "Edit draft", exact: true }).click();
    await page.getByLabel("When should this go out?").selectOption("exact");
    await page
      .getByLabel(/Date and time/)
      .fill(new Date(Date.now() + 7 * 86400_000).toISOString().slice(0, 16));
    await page.getByRole("button", { name: "Schedule post" }).click();
    await expect(
      page.getByRole("button", { name: "Check scheduling outcome" }),
    ).toBeEnabled();
    const saved = await page.evaluate(
      (key) => JSON.parse(localStorage.getItem(key)!),
      key,
    );
    expect(saved.uncertain).toBe(true);
    expect(saved.id).toBe(requests[0].request_id);
    const lookup = await context.request.get(
      `/api/v1/schedule-requests/${saved.id}`,
    );
    expect(lookup.ok(), await lookup.text()).toBe(true);
    const receipt = (await lookup.json()).data;
    expect(receipt.outcome).toBe("rejected");
    expect(receipt.error.code).toBe("provider_format_invalid");
    expect(receipt.publications).toEqual([]);
    const replay = await context.request.post(
      `/api/v1/posts/${post.id}/schedule?view=summary`,
      {
        data: requests[0],
        headers: {
          "X-Beweft-Request": "1",
          Origin: process.env.BEWEFT_TEST_URL!,
        },
      },
    );
    expect(replay.status()).toBe(400);
    expect((await replay.json()).error.code).toBe("provider_format_invalid");
    await page
      .getByRole("button", { name: "Check scheduling outcome" })
      .click();
    await expect(
      page.getByText(/original schedule was not made/),
    ).toBeVisible();
    await expect(page.getByLabel("Post text", { exact: true })).toBeEnabled();
    await page
      .getByLabel("Post text", { exact: true })
      .fill("Corrected format");
    await page.getByRole("button", { name: "Schedule post" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    expect(requests).toHaveLength(2);
    expect(requests[1].request_id).not.toBe(saved.id);
    expect(requests[1].version).toBe(saved.post.version + 1);
    const publications = await context.request.get(
      `/api/v1/publications?brand_id=${brand.id}&view=summary`,
    );
    expect(publications.ok(), await publications.text()).toBe(true);
    expect((await publications.json()).data).toHaveLength(1);
  });
});
