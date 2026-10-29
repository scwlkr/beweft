import { test, expect } from "@playwright/test";
import { withWorkspace } from "./helpers";

test("expired upload requires an attested reset and never automatically retries (controlled API fixtures)", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Recovery fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Video fixture",
      timezone: "UTC",
    });
    const post = await write("/posts", {
      brand_id: brand.id,
      text: "Isolated recovery",
      title: "Expired video",
      media_ids: [],
      variants: [],
    });
    const [publication] = await write(`/posts/${post.id}/schedule`, {
      version: post.version,
      channel_ids: [channel.id],
      scheduled_at: new Date(Date.now() + 86400000).toISOString(),
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.route("**/publications?*", (route) =>
      route.fulfill({
        json: {
          data: [{ ...publication, state: "failed", provider: "youtube" }],
        },
      }),
    );
    await page.route(`**/publications/${publication.id}/attempts`, (route) =>
      route.fulfill({
        json: {
          data: [
            {
              id: "owner-fixture",
              state: "failed",
              started_at: publication.created_at,
              receipt: {
                verification: "owner_attestation",
                action: "reconcile",
                detail: {
                  resolution: "not_published",
                  note: "Checked account and video does not exist.",
                },
              },
            },
          ],
        },
      }),
    );
    let expired = true,
      resets = 0,
      retries = 0;
    await page.route(
      `**/publications/${publication.id}/provider-status`,
      (route) =>
        route.fulfill({
          json: {
            data: {
              session_expired: expired,
              reset_allowed: expired,
              reason: expired ? "Upload session expired" : null,
            },
          },
        }),
    );
    await page.route(
      `**/publications/${publication.id}/reset-expired-session`,
      (route) => {
        resets++;
        expired = false;
        return route.fulfill({
          json: {
            data: {
              state: "failed",
              session_reset: true,
              next_action: "Retry to start a new upload session",
            },
          },
        });
      },
    );
    await page.route(`**/publications/${publication.id}/retry`, (route) => {
      retries++;
      return route.abort();
    });
    await page.goto("/#queue");
    await page.getByRole("button", { name: "View details" }).click();
    await expect(page.getByText("Owner attestation · failed")).toBeVisible();
    await expect(
      page.getByText(
        "Reconciliation / completion note: Checked account and video does not exist.",
      ),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Retry publication" }),
    ).toBeDisabled();
    const reset = page.getByRole("button", {
      name: "Reset expired upload session",
      exact: true,
    });
    await expect(reset).toBeDisabled();
    await page
      .getByRole("checkbox", { name: /I verified that this video/ })
      .check();
    await reset.click();
    await expect(
      page.getByText("Retry to start a new upload session", { exact: true }),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Retry publication" }),
    ).toBeEnabled();
    expect(resets).toBe(1);
    expect(retries).toBe(0);
  });
});

test("Mastodon reply keeps the original audience visible (controlled API fixture)", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Private reply fixture" });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.route("**/inbox?*", (route) =>
      route.fulfill({
        json: {
          data: {
            availability: "partial",
            items: [
              {
                id: "fixture-inbox",
                channel_id: "fixture-channel",
                channel_name: "Mastodon fixture",
                provider: "mastodon",
                item: {
                  id: "fixture-item",
                  author: "Author",
                  text: "Private original message",
                  reply_context: {
                    visibility: "private",
                    recipients: [
                      { id: "participant", acct: "friend@example.social" },
                    ],
                  },
                },
                read_at: null,
                synced_at: new Date().toISOString(),
                can_reply: true,
              },
            ],
          },
        },
      }),
    );
    await page.goto("/#inbox");
    await page.getByRole("button", { name: "Reply", exact: true }).click();
    await expect(page.getByText("Original audience: private")).toBeVisible();
    await expect(
      page.getByText("Participants: @friend@example.social"),
    ).toBeVisible();
    await expect(
      page.getByText(/expanding the recipient list is blocked/),
    ).toBeVisible();
    await expect(
      page.getByText(
        /Your reply is sent directly to the listed original participants/,
      ),
    ).toBeVisible();
    // No reply is submitted; provider privacy behavior is separately tested in Rust.
  });
});
