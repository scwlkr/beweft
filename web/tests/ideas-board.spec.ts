import { expect, test } from "@playwright/test";
import { deferred, withWorkspace } from "./helpers";

test("organize ideas by stage, group and tags without losing ideas when a group is deleted", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Idea board fixture" });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.goto("/#ideas");
    await expect(
      page.getByRole("button", { name: "Manage groups" }),
    ).toBeEnabled();
    await page.getByRole("button", { name: "Manage groups" }).click();
    await page.getByLabel("New group name").fill("Summer campaign");
    await page.getByRole("button", { name: "Add group" }).click();
    await expect(page.getByLabel("Name for Summer campaign")).toBeVisible();
    await page.getByRole("button", { name: "Close dialog" }).click();
    await page
      .getByRole("button", { name: "Capture an idea", exact: true })
      .first()
      .click();
    await page.getByLabel("Idea title").fill("A sunny launch");
    await page.getByLabel("Notes", { exact: true }).fill("A reusable seed");
    await page.getByLabel("Stage", { exact: true }).selectOption("in_progress");
    await page
      .getByLabel("Group", { exact: true })
      .selectOption({ label: "Summer campaign" });
    await page.getByLabel("Tags").fill("summer\nBehind, scenes\nSUMMER\nall");
    await page.getByRole("button", { name: "Save idea" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    const progress = page.getByRole("region", { name: "In progress" });
    await expect(
      progress.getByRole("heading", { name: "A sunny launch" }),
    ).toBeVisible();
    await expect(progress.getByText("Summer campaign")).toBeVisible();
    await expect(progress.getByText("summer", { exact: true })).toBeVisible();
    await expect(
      progress.getByText("Behind, scenes", { exact: true }),
    ).toBeVisible();
    await page
      .getByLabel("Filter by tag")
      .selectOption({ label: "Behind, scenes" });
    await expect(
      progress.getByRole("heading", { name: "A sunny launch" }),
    ).toBeVisible();
    await page.getByLabel("Filter by group").selectOption("ungrouped");
    await expect(
      progress.getByRole("heading", { name: "A sunny launch" }),
    ).toHaveCount(0);
    await page.getByLabel("Filter by group").selectOption("all");
    await page.getByLabel("Filter by tag").selectOption({ label: "all" });
    await expect(page.getByLabel("Filter by tag")).toHaveValue("tag:all");
    await page.getByLabel("Filter by tag").selectOption({ label: "All tags" });
    await page.getByLabel("Move A sunny launch").selectOption("ready");
    const ready = page.getByRole("region", { name: "Ready", exact: true });
    await expect(
      ready.getByRole("heading", { name: "A sunny launch" }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Manage groups" }).click();
    await page.getByLabel("Name for Summer campaign").fill("Bright ideas");
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Save", exact: true })
      .click();
    await expect(page.getByLabel("Name for Bright ideas")).toBeVisible();
    await page
      .getByRole("button", { name: "Delete group Bright ideas" })
      .click();
    await expect(page.getByLabel("Name for Bright ideas")).toHaveCount(0);
    await page.getByRole("button", { name: "Close dialog" }).click();
    await expect(
      ready.getByRole("heading", { name: "A sunny launch" }),
    ).toBeVisible();
    await page.getByLabel("Filter by group").selectOption("ungrouped");
    await expect(
      ready.getByRole("heading", { name: "A sunny launch" }),
    ).toBeVisible();
    await ready.getByRole("button", { name: "Create post" }).click();
    await expect(page.getByLabel("Post text", { exact: true })).toHaveValue(
      "A reusable seed",
    );
    await page.getByRole("button", { name: "Close dialog" }).click();
    await page.setViewportSize({ width: 390, height: 844 });
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
  });
});

test("unavailable groups do not block idea edits or clear their existing group", async () => {
  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Idea group outage" });
    const group = await write("/idea-groups", {
      brand_id: brand.id,
      name: "Protected group",
    });
    const idea = await write("/ideas", {
      brand_id: brand.id,
      title: "Outage idea",
      text: "Before",
      group_id: group.id,
      tags: ["one"],
    });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    let failed = true;
    await page.route("**/api/v1/idea-groups?*", (route) =>
      failed
        ? route.fulfill({
            status: 503,
            json: {
              error: { code: "unavailable", message: "Groups are unavailable" },
            },
          })
        : route.continue(),
    );
    await page.goto("/#ideas");
    await expect(
      page.getByText(/Idea groups could not be loaded/),
    ).toBeVisible();
    await page.getByRole("button", { name: "Edit Outage idea" }).click();
    await expect(page.getByLabel("Group", { exact: true })).toBeDisabled();
    await page.getByLabel("Notes", { exact: true }).fill("After");
    await page.getByRole("button", { name: "Save idea" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    const response = await context.request.get(
      `/api/v1/ideas?brand_id=${brand.id}`,
    );
    expect(
      (await response.json()).data.find(
        (row: { id: string }) => row.id === idea.id,
      ).group_id,
    ).toBe(group.id);
    failed = false;
    await page.getByRole("button", { name: "Retry groups" }).click();
    await expect(
      page
        .getByRole("region", { name: "Inbox" })
        .getByText("Protected group", { exact: true }),
    ).toBeVisible();
  });
});

test("late group data from an old brand is not shown after a switch", async () => {
  await withWorkspace(async (page, context, write) => {
    const alpha = await write("/brands", { name: "Idea Alpha" });
    const beta = await write("/brands", { name: "Idea Beta" });
    await write("/idea-groups", { brand_id: alpha.id, name: "Old group" });
    await write("/idea-groups", { brand_id: beta.id, name: "New group" });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      alpha.id,
    );
    const started = deferred(),
      release = deferred();
    await page.route(
      `**/api/v1/idea-groups?brand_id=${alpha.id}`,
      async (route) => {
        const response = await route.fetch();
        started.resolve();
        await release.promise;
        await route.fulfill({ response }).catch(() => {});
      },
    );
    try {
      await page.goto("/#ideas");
      await started.promise;
      await page.getByLabel("Current brand").selectOption(beta.id);
      await expect(
        page.getByRole("button", { name: "Manage groups" }),
      ).toBeEnabled();
      release.resolve();
      await page.getByRole("button", { name: "Manage groups" }).click();
      await expect(page.getByLabel("Name for New group")).toBeVisible();
      await expect(page.getByLabel("Name for Old group")).toHaveCount(0);
    } finally {
      release.resolve();
    }
  });
});
