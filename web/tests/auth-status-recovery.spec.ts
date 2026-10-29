import { expect, test } from "@playwright/test";
import { withWorkspace } from "./helpers";

test("sign-in status offers retry through a persistent outage and recovers", async () => {
  await withWorkspace(async (page) => {
    let statusUnavailable = true,
      rejectSession = true,
      statusRequests = 0;
    await page.route("**/api/v1/auth/me", (route) =>
      rejectSession
        ? route.fulfill({
            status: 401,
            json: { error: { code: "unauthorized", message: "Sign in" } },
          })
        : route.continue(),
    );
    await page.route("**/api/v1/auth/status", (route) => {
      statusRequests++;
      return statusUnavailable
        ? route.fulfill({
            status: 503,
            json: {
              error: {
                code: "unavailable",
                message: "Status temporarily unavailable",
              },
            },
          })
        : route.continue();
    });
    await page.goto("/#drafts");
    await expect(
      page.getByText("Status temporarily unavailable"),
    ).toBeVisible();
    const beforeRetry = statusRequests;
    const repeatedFailure = page.waitForResponse(
      (response) =>
        response.url().endsWith("/api/v1/auth/status") &&
        response.status() === 503,
    );
    await page.getByRole("button", { name: "Retry", exact: true }).click();
    await repeatedFailure;
    await expect.poll(() => statusRequests).toBeGreaterThan(beforeRetry);
    await expect(
      page.getByText("Status temporarily unavailable"),
    ).toBeVisible();
    statusUnavailable = false;
    await page.getByRole("button", { name: "Retry", exact: true }).click();
    await expect(page.getByLabel("Email address")).toBeVisible();
    rejectSession = false;
    await page.getByLabel("Email address").fill(process.env.BEWEFT_TEST_EMAIL!);
    await page
      .getByLabel("Password", { exact: true })
      .fill(process.env.BEWEFT_TEST_PASSWORD!);
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(page.getByLabel("Current brand")).toBeVisible();
  });
});
