import { expect, test } from "@playwright/test";
import { withWorkspace } from "./helpers";

test("busy and changed reply observations require a new read and attestation", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Reply observation fixture" });
    const itemId = crypto.randomUUID(),
      channelId = crypto.randomUUID(),
      requestId = crypto.randomUUID();
    await context.addInitScript(
      (id: string) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.route("**/inbox?*", (route) =>
      route.fulfill({
        json: {
          data: {
            availability: "partial",
            items: [
              {
                id: itemId,
                channel_id: channelId,
                channel_name: "Fixture",
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
    let row = {
      id: requestId,
      inbox_id: itemId,
      text: "Original reply",
      revision: 1,
      state: "uncertain",
      error: null,
      receipt: null as unknown,
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
    };
    await page.route(`**/inbox/${itemId}/replies`, (route) =>
      route.fulfill({ json: { data: [row] } }),
    );
    const submissions: any[] = [];
    await page.route(
      `**/inbox/${itemId}/replies/${requestId}/resolve`,
      (route) => {
        const body = route.request().postDataJSON();
        submissions.push(body);
        if (submissions.length === 1)
          return route.fulfill({
            status: 409,
            json: {
              error: {
                code: "provider_busy",
                message: "An active reply is still running.",
              },
            },
          });
        if (submissions.length === 2) {
          row = { ...row, revision: 2 };
          return route.fulfill({
            status: 409,
            json: {
              error: {
                code: "reply_observation_changed",
                message: "Reply history changed.",
              },
            },
          });
        }
        row = {
          ...row,
          state: "failed",
          revision: 3,
          receipt: { verification: "owner_attestation" },
        };
        return route.fulfill({ json: { data: row } });
      },
    );
    await page.goto("/#inbox");
    await page.getByRole("button", { name: "Reply", exact: true }).click();
    await expect(
      page.getByText("Server reply outcome: uncertain"),
    ).toBeVisible();
    async function review() {
      await page
        .getByLabel("Verified reply outcome")
        .selectOption("not_published");
      await page
        .getByLabel("Reply verification note")
        .fill("Inspected the original conversation.");
      await page
        .getByRole("checkbox", { name: /I checked the network/ })
        .check();
      await expect(
        page.getByRole("button", { name: "Record reply attestation" }),
      ).toBeDisabled();
      await page
        .getByRole("checkbox", {
          name: /I confirmed the original sender has stopped/,
        })
        .check();
      await page
        .getByRole("button", { name: "Record reply attestation" })
        .click();
    }
    await review();
    await expect(
      page.getByText(/An active reply is still running/),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Record reply attestation" }),
    ).toHaveCount(0);
    await page
      .getByRole("button", { name: "Check existing reply outcome" })
      .click();
    await expect(page.getByLabel("Verified reply outcome")).toHaveValue("");
    await review();
    await expect(page.getByText(/Reply history changed/)).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Record reply attestation" }),
    ).toHaveCount(0);
    await page
      .getByRole("button", { name: "Check existing reply outcome" })
      .click();
    await expect(page.getByLabel("Verified reply outcome")).toHaveValue("");
    await review();
    await expect(page.getByText("Server reply outcome: failed")).toBeVisible();
    expect(submissions.map((body) => body.observed_revision)).toEqual([
      1, 1, 2,
    ]);
    expect(submissions.every((body) => body.sender_stopped === true)).toBe(
      true,
    );
    await expect(
      page.getByText(/Owner attestation: this outcome was recorded/),
    ).toBeVisible();
  });
});
