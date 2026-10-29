import { expect, test } from "@playwright/test";
import { withWorkspace } from "./helpers";

test("Google post options keep local times and remove incompatible fields", async () => {
  await withWorkspace(async (page, context, write) => {
    await page.setViewportSize({ width: 390, height: 844 });
    const brand = await write("/brands", { name: "Google options fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Location fixture",
      timezone: "UTC",
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.route("**/channels?*", (route) =>
      route.fulfill({
        json: { data: [{ ...channel, provider: "google_business" }] },
      }),
    );
    await page.goto("/#drafts");
    await page
      .getByRole("button", { name: "Create post", exact: true })
      .click();
    await page.getByRole("button", { name: "Location fixture" }).click();
    await page.getByRole("tab", { name: "Location fixture" }).click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Location fixture text").fill("Local event");
    await dialog.getByLabel("Post type").selectOption("EVENT");
    await expect(dialog).toContainText(
      "without converting from your browser’s timezone",
    );
    await dialog.getByLabel("Language code").fill("en-US");
    await dialog.getByLabel("Event title").fill("Fixture event");
    await dialog.getByLabel("Event start date").fill("2027-02-11");
    await dialog.getByLabel("Event start time").fill("09:15");
    await dialog.getByLabel("Event end date").fill("2027-02-12");
    await dialog.getByLabel("Event end time").fill("17:45");
    await dialog.getByLabel("Call to action").selectOption("BOOK");
    await dialog
      .getByLabel("Call to action URL")
      .fill("https://example.test/book");
    let saved = page.waitForRequest(
      (request) =>
        request.url().endsWith("/api/v1/posts") && request.method() === "POST",
    );
    await dialog.getByRole("button", { name: "Save draft" }).click();
    const eventBody = (await saved).postDataJSON();
    expect(eventBody.variants[0].options).toEqual({
      topic_type: "EVENT",
      language_code: "en-US",
      event: {
        title: "Fixture event",
        start_date: "2027-02-11",
        start_time: "09:15",
        end_date: "2027-02-12",
        end_time: "17:45",
      },
      call_to_action: { type: "BOOK", url: "https://example.test/book" },
    });
    await expect(dialog).toHaveCount(0);
    await page.getByRole("button", { name: "Edit draft" }).click();
    await page.getByRole("tab", { name: "Location fixture" }).click();
    await expect(dialog.getByLabel("Event start time")).toHaveValue("09:15");
    await dialog.getByLabel("Post type").selectOption("OFFER");
    await expect(dialog.getByLabel("Call to action")).toHaveCount(0);
    await dialog.getByLabel("Coupon code").fill("LOCAL");
    await dialog
      .getByLabel("Online redemption URL")
      .fill("https://example.test/redeem");
    await dialog
      .getByLabel("Offer terms and conditions")
      .fill("While available");
    saved = page.waitForRequest(
      (request) =>
        /\/api\/v1\/posts\//.test(request.url()) &&
        request.method() === "PATCH",
    );
    await dialog.getByRole("button", { name: "Save draft" }).click();
    const offerBody = (await saved).postDataJSON();
    expect(offerBody.variants[0].options).toEqual({
      topic_type: "OFFER",
      language_code: "en-US",
      event: eventBody.variants[0].options.event,
      offer: {
        coupon_code: "LOCAL",
        redeem_online_url: "https://example.test/redeem",
        terms_conditions: "While available",
      },
    });
    await expect(dialog).toHaveCount(0);
    await page.getByRole("button", { name: "Edit draft" }).click();
    await page.getByRole("tab", { name: "Location fixture" }).click();
    await dialog.getByLabel("Post type").selectOption("STANDARD");
    await dialog.getByLabel("Call to action").selectOption("CALL");
    await expect(dialog.getByLabel("Call to action URL")).toHaveCount(0);
    saved = page.waitForRequest(
      (request) =>
        /\/api\/v1\/posts\//.test(request.url()) &&
        request.method() === "PATCH",
    );
    await dialog.getByRole("button", { name: "Save draft" }).click();
    expect((await saved).postDataJSON().variants[0].options).toEqual({
      topic_type: "STANDARD",
      language_code: "en-US",
      call_to_action: { type: "CALL" },
    });
  });
});

test("Google recovery fences both owner outcomes and never retries automatically", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Google recovery fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Location fixture",
      timezone: "UTC",
    });
    const post = await write("/posts", {
      brand_id: brand.id,
      title: "Fixture post",
      text: "Local fixture",
      variants: [],
    });
    const [original] = await write(`/posts/${post.id}/schedule`, {
      version: post.version,
      channel_ids: [channel.id],
      scheduled_at: new Date(Date.now() + 86400000).toISOString(),
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    let publication = {
      ...original,
      provider: "google_business",
      state: "uncertain",
    };
    const first = {
      phase: "processing",
      attempt_id: crypto.randomUUID(),
      checkpoint_id: crypto.randomUUID(),
      post_name: "accounts/1/locations/2/localPosts/3",
      retry_allowed: false,
      reason: "Google is still processing.",
    };
    const second = {
      ...first,
      attempt_id: crypto.randomUUID(),
      checkpoint_id: crypto.randomUUID(),
      reason: "Review the newer operation.",
    };
    let current: typeof first | null = first;
    const reconciles: any[] = [];
    let retries = 0,
      providerChecks = 0;
    await page.route("**/publications?*", (route) =>
      route.fulfill({ json: { data: [publication] } }),
    );
    await page.route(`**/publications/${original.id}/attempts`, (route) =>
      route.fulfill({ json: { data: [] } }),
    );
    await page.route(
      `**/publications/${original.id}/provider-status`,
      (route) =>
        route.fulfill({ json: { data: { google_delivery: current } } }),
    );
    await page.route(
      `**/publications/${original.id}/provider-reconcile`,
      (route) => {
        providerChecks++;
        if (providerChecks === 1)
          return route.fulfill({
            status: 404,
            json: { error: { code: "not_found", message: "Post not found" } },
          });
        return route.fulfill({
          json: { data: { state: "uncertain", reason: "Still processing." } },
        });
      },
    );
    await page.route(`**/publications/${original.id}/reconcile`, (route) => {
      reconciles.push(route.request().postDataJSON());
      if (reconciles.length === 1) {
        current = second;
        return route.fulfill({
          status: 409,
          json: { error: { code: "conflict", message: "Attempt changed" } },
        });
      }
      current = { ...second, retry_allowed: true };
      publication = {
        ...publication,
        state: "failed",
        updated_at: new Date().toISOString(),
      };
      return route.fulfill({ json: { data: publication } });
    });
    await page.route(`**/publications/${original.id}/retry`, (route) => {
      retries++;
      publication = { ...publication, state: "queued" };
      return route.fulfill({ json: { data: publication } });
    });
    await page.goto("/#queue");
    await page.getByRole("button", { name: "Reconcile outcome" }).click();
    const dialog = page.getByRole("dialog");
    await expect(dialog).toContainText(first.checkpoint_id);
    await expect(dialog).toContainText("a 404 alone does not prove");
    await expect(
      dialog.getByRole("button", { name: "Cancel publication" }),
    ).toHaveCount(0);
    await expect(
      dialog.getByRole("button", { name: "Retry publication" }),
    ).toHaveCount(0);
    await dialog
      .getByRole("button", { name: "Check provider outcome" })
      .click();
    await expect(dialog).toContainText("provider check was not confirmed");
    await expect(dialog).toContainText(first.checkpoint_id);
    await expect(
      dialog.getByRole("button", { name: "Retry publication" }),
    ).toHaveCount(0);
    await dialog
      .getByRole("button", { name: "Check provider outcome" })
      .click();
    await expect(dialog).toContainText("Still processing.");
    await expect(dialog).toContainText(first.checkpoint_id);
    await dialog.getByLabel("Verified outcome").selectOption("published");
    await dialog.getByLabel("Verification note").fill("Found the post");
    await dialog
      .getByLabel("Published post URL (optional)")
      .fill("https://example.test/post");
    await expect(
      dialog.getByRole("button", { name: "Record outcome" }),
    ).toBeDisabled();
    await dialog
      .getByRole("checkbox", { name: /original sender has stopped/ })
      .check();
    await dialog
      .getByRole("checkbox", { name: /inspected Google and confirmed/ })
      .check();
    await dialog.getByRole("button", { name: "Record outcome" }).click();
    await expect(dialog).toContainText(second.checkpoint_id);
    await dialog.getByLabel("Verified outcome").selectOption("not_published");
    await dialog
      .getByLabel("Verification note")
      .fill("Confirmed Google canceled it");
    await dialog
      .getByRole("checkbox", { name: /original sender has stopped/ })
      .check();
    const inspected = dialog.getByRole("checkbox", {
      name: /operation definitively failed/,
    });
    await expect(inspected).toHaveAccessibleName(
      /An absent post or stopped local sender alone does not prove/,
    );
    await inspected.check();
    await dialog.getByRole("button", { name: "Record outcome" }).click();
    await expect(
      dialog.getByRole("button", { name: "Retry publication" }),
    ).toBeEnabled();
    expect(retries).toBe(0);
    expect(providerChecks).toBe(2);
    expect(reconciles).toEqual([
      {
        resolution: "published",
        note: "Found the post",
        external_url: "https://example.test/post",
        observed_attempt_id: first.attempt_id,
        observed_checkpoint_id: first.checkpoint_id,
        sender_stopped: true,
        provider_inspected: true,
      },
      {
        resolution: "not_published",
        note: "Confirmed Google canceled it",
        observed_attempt_id: second.attempt_id,
        observed_checkpoint_id: second.checkpoint_id,
        sender_stopped: true,
        provider_inspected: true,
      },
    ]);
    await dialog.getByRole("button", { name: "Retry publication" }).click();
    expect(retries).toBe(1);
  });
});

test("missing Google status never invents a safe retry or cancellation", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Google unknown fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Location fixture",
      timezone: "UTC",
    });
    const post = await write("/posts", {
      brand_id: brand.id,
      text: "Local fixture",
      variants: [],
    });
    const [original] = await write(`/posts/${post.id}/schedule`, {
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
          data: [{ ...original, provider: "google_business", state: "failed" }],
        },
      }),
    );
    await page.route(`**/publications/${original.id}/attempts`, (route) =>
      route.fulfill({ json: { data: [] } }),
    );
    let status: any = {};
    await page.route(
      `**/publications/${original.id}/provider-status`,
      (route) =>
        status === "error"
          ? route.fulfill({
              status: 503,
              json: {
                error: { code: "unavailable", message: "Status unavailable" },
              },
            })
          : route.fulfill({ json: { data: status } }),
    );
    await page.goto("/#queue");
    await page.getByRole("button", { name: "View details" }).click();
    const dialog = page.getByRole("dialog");
    await expect(dialog).toContainText("No current Google delivery checkpoint");
    await expect(
      dialog.getByRole("button", { name: "Retry publication" }),
    ).toBeDisabled();
    await expect(
      dialog.getByRole("button", { name: "Cancel publication" }),
    ).toHaveCount(0);
    await expect(
      dialog.getByRole("button", { name: "Record outcome" }),
    ).toHaveCount(0);
    status = { google_delivery: null };
    await dialog.getByRole("button", { name: "Check delivery status" }).click();
    await expect(dialog).toContainText("No current Google delivery checkpoint");
    await expect(
      dialog.getByRole("button", { name: "Retry publication" }),
    ).toBeDisabled();
    status = {
      google_delivery: {
        phase: "rejected",
        attempt_id: crypto.randomUUID(),
        checkpoint_id: crypto.randomUUID(),
        post_name: null,
        retry_allowed: false,
        reason: "No retry signal.",
      },
    };
    await dialog.getByRole("button", { name: "Check delivery status" }).click();
    await expect(dialog).toContainText("No retry signal.");
    await expect(
      dialog.getByRole("button", { name: "Retry publication" }),
    ).toBeDisabled();
    status.google_delivery.retry_allowed = true;
    await dialog.getByRole("button", { name: "Check delivery status" }).click();
    await expect(
      dialog.getByRole("button", { name: "Retry publication" }),
    ).toBeEnabled();
    status = "error";
    await dialog.getByRole("button", { name: "Check delivery status" }).click();
    await expect(dialog).toContainText("Status unavailable");
    await expect(
      dialog.getByRole("button", { name: "Retry publication" }),
    ).toBeDisabled();
    await expect(
      dialog.getByRole("button", { name: "Cancel publication" }),
    ).toHaveCount(0);
  });
});

test("Google-family removal explains shared impact and uses the current removal ID", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Google family fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Location fixture",
      timezone: "UTC",
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    const scope = "https://www.googleapis.com/auth/business.manage";
    await page.route("**/channels?*", (route) =>
      route.fulfill({
        json: {
          data: [
            {
              ...channel,
              provider: "google_business",
              connection_status: "connected",
              capabilities: {
                permissions: {
                  source: "reported",
                  reported_scopes: [scope],
                  actions: {
                    publish: {
                      status: "unknown",
                      required_scopes: [scope],
                      missing_scopes: null,
                    },
                  },
                },
              },
            },
          ],
        },
      }),
    );
    await page.route("**/providers", (route) =>
      route.fulfill({
        json: {
          data: [
            {
              id: "google_business",
              name: "Google Business Profile",
              implementation: "available",
              configuration: "ready",
              execution: "direct",
            },
            {
              id: "youtube",
              name: "YouTube",
              implementation: "available",
              configuration: "ready",
              execution: "direct",
            },
          ],
        },
      }),
    );
    await page.route("**/provider-apps", (route) =>
      route.fulfill({ json: { data: [] } }),
    );
    const first = crypto.randomUUID(),
      second = crypto.randomUUID();
    let pendingId: string | null = first;
    const acknowledgements: { provider: string; body: unknown }[] = [];
    await page.route(`**/channels/${channel.id}/disconnect`, (route) =>
      route.fulfill({
        json: {
          data: {
            disconnected: true,
            provider_revoked: false,
            channels_requiring_reconnection: 1,
            revocation_id: first,
            next_action: "Review Google access.",
          },
        },
      }),
    );
    for (const provider of ["google_business", "youtube"]) {
      await page.route(`**/connections/${provider}/removal-status`, (route) =>
        route.fulfill({
          json: { data: { provider, revocation_id: pendingId } },
        }),
      );
      await page.route(
        `**/connections/${provider}/acknowledge-revocation`,
        (route) => {
          acknowledgements.push({
            provider,
            body: route.request().postDataJSON(),
          });
          if (acknowledgements.length === 1) {
            pendingId = second;
            return route.fulfill({
              status: 409,
              json: {
                error: { code: "conflict", message: "Removal changed." },
              },
            });
          }
          pendingId = null;
          return route.fulfill({
            json: { data: { revocation_acknowledged: true } },
          });
        },
      );
    }
    await page.goto("/#connections");
    await page.getByText("Account permissions").click();
    await expect(page.getByText(`Reported scopes: ${scope}.`)).toBeVisible();
    await expect(
      page.getByText(/Unknown · Provider has not confirmed/),
    ).toBeVisible();
    await page.getByRole("button", { name: "Disconnect", exact: true }).click();
    const dialog = page.getByRole("dialog");
    await expect(dialog).toContainText("unsent queued deliveries");
    await expect(dialog).toContainText(
      "current and prior Google projects and accounts",
    );
    await expect(dialog).toContainText(
      "keep their stored credentials and queues",
    );
    await expect(dialog).toContainText("does not revoke access at Google");
    await dialog
      .getByRole("checkbox", { name: /I understand the selected queue/ })
      .check();
    await dialog.getByRole("button", { name: "Disconnect channel" }).click();
    await dialog
      .getByRole("button", { name: "Review provider removal status" })
      .click();
    await expect(dialog).toContainText(first);
    await expect(dialog).toContainText(
      "inspect your other beweft YouTube and Google Business Profile channels",
    );
    await expect(dialog).toContainText(
      "Remove only the relevant beweft access",
    );
    await expect(
      dialog.getByRole("button", { name: "Save provider app" }),
    ).toBeDisabled();
    await expect(
      dialog.getByRole("button", { name: "Reconnect account" }),
    ).toBeDisabled();
    await dialog.getByRole("button", { name: "Close dialog" }).click();
    await page
      .getByRole("button", { name: /YouTube.*Ready to connect/ })
      .click();
    await expect(dialog).toContainText(first);
    await expect(
      dialog.getByRole("button", { name: "Save provider app" }),
    ).toBeDisabled();
    await expect(
      dialog.getByRole("button", { name: "Authorize account" }),
    ).toBeDisabled();
    await dialog
      .getByRole("checkbox", { name: /I removed beweft access/ })
      .check();
    await dialog
      .getByRole("button", { name: "Confirm provider access removed" })
      .click();
    await expect(dialog).toContainText("Removal changed.");
    await dialog.getByRole("button", { name: "Check removal status" }).click();
    await expect(dialog).toContainText(second);
    await expect(
      dialog.getByRole("checkbox", { name: /I removed beweft access/ }),
    ).not.toBeChecked();
    await dialog
      .getByRole("checkbox", { name: /I removed beweft access/ })
      .check();
    await dialog
      .getByRole("button", { name: "Confirm provider access removed" })
      .click();
    await expect(dialog).toContainText("Your removal attestation was recorded");
    expect(acknowledgements).toEqual([
      {
        provider: "youtube",
        body: { access_removed: true, revocation_id: first },
      },
      {
        provider: "youtube",
        body: { access_removed: true, revocation_id: second },
      },
    ]);
  });
});

test("Google destination selection sends the discovered account and location pair", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", {
      name: "Google destination fixture",
    });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Location fixture",
      timezone: "UTC",
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    const flowId = crypto.randomUUID();
    const pair = "accounts/123/locations/456";
    await page.route(`**/connection-flows/${flowId}`, (route) =>
      route.fulfill({
        json: {
          data: {
            id: flowId,
            brand_id: brand.id,
            provider: "google_business",
            destinations: [
              {
                id: pair,
                name: "Fixture location",
                username: "Fixture account",
              },
            ],
          },
        },
      }),
    );
    const bodies: any[] = [];
    await page.route(`**/connection-flows/${flowId}/complete`, (route) => {
      bodies.push(route.request().postDataJSON());
      return route.fulfill({ json: { data: channel } });
    });
    await page.goto(`/?connection_flow=${flowId}#connections`);
    const dialog = page.getByRole("dialog");
    await expect(dialog).toContainText(
      "location and the Google account that manages it",
    );
    await expect(
      dialog.getByRole("option", {
        name: "Fixture location · Fixture account",
      }),
    ).toHaveAttribute("value", pair);
    await dialog.getByLabel("Destination").selectOption(pair);
    await dialog.getByRole("button", { name: "Finish connection" }).click();
    await expect(dialog).toHaveCount(0);
    expect(bodies).toHaveLength(1);
    expect(bodies[0].destination_id).toBe(pair);
  });
});
