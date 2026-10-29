import { createCipheriv, randomBytes, randomUUID } from "node:crypto";
import { execFileSync } from "node:child_process";
import { expect, test } from "@playwright/test";
import { withWorkspace } from "./helpers";

// This test creates a synthetic checkpoint in the harness-owned database. The
// format-fixture runner verifies the OS network sandbox, enables the matching
// provider-fixture build, and rejects any unexpected provider request.
test("Google composer and exact-ID recovery work with the real API", async () => {
  test.skip(
    process.env.BEWEFT_TEST_FORMAT_FIXTURE !== "1",
    "Requires the loopback-only provider fixture harness",
  );
  if (
    process.env.BEWEFT_REQUIRE_PROVIDER_FIXTURES !== "true" ||
    process.env.BEWEFT_PROVIDER_TEST_SANDBOX !== "1" ||
    !/^http:\/\/127\.0\.0\.1:\d+$/.test(
      process.env.BEWEFT_PROVIDER_FIXTURE_ORIGIN ?? "",
    ) ||
    !/^beweft_e2e_[a-f0-9]{12}$/.test(process.env.BEWEFT_TEST_DB_NAME ?? "") ||
    !/^[a-f0-9]{64}$/.test(process.env.BEWEFT_ENCRYPTION_KEY ?? "")
  )
    throw new Error("Requires the guarded disposable provider harness");
  execFileSync(
    "python3",
    [
      "-c",
      "import errno,socket,sys; s=socket.socket(); s.settimeout(1); sys.exit(0 if s.connect_ex(('192.0.2.1',9)) in (errno.EPERM,errno.EACCES) else 1)",
    ],
    { stdio: "ignore" },
  );
  const db = process.env.BEWEFT_TEST_DB_NAME!;
  const sql = (query: string) =>
    execFileSync(
      "docker",
      [
        "exec",
        process.env.BEWEFT_TEST_POSTGRES_CONTAINER ?? "beweft-dev-postgres",
        "psql",
        "-X",
        "-qAt",
        "-U",
        process.env.BEWEFT_TEST_DB_USER ?? "beweft",
        "-d",
        db,
        "-v",
        "ON_ERROR_STOP=1",
        "-c",
        query,
      ],
      { stdio: "ignore" },
    );
  const encryptedCheckpoint = (publicationId: string, checkpoint: unknown) => {
    if (!/^[0-9a-f-]{36}$/.test(publicationId))
      throw new Error("Invalid fixture ID");
    const nonce = randomBytes(12);
    const cipher = createCipheriv(
      "chacha20-poly1305",
      Buffer.from(process.env.BEWEFT_ENCRYPTION_KEY!, "hex"),
      nonce,
      { authTagLength: 16 },
    );
    cipher.setAAD(Buffer.from(publicationId.replaceAll("-", ""), "hex"));
    const encrypted = Buffer.concat([
      cipher.update(JSON.stringify(checkpoint), "utf8"),
      cipher.final(),
    ]);
    return Buffer.concat([nonce, encrypted, cipher.getAuthTag()]).toString(
      "base64",
    );
  };

  await withWorkspace(async (page, context, write) => {
    const brand = await write("/brands", { name: "Google API UI fixture" });
    const channel = await write("/channels", {
      brand_id: brand.id,
      provider: "manual",
      name: "Local location",
      timezone: "UTC",
    });
    if (!/^[0-9a-f-]{36}$/.test(channel.id))
      throw new Error("Invalid fixture channel");
    sql(
      `UPDATE channels SET provider='google_business', target_platform=NULL, connection_status='connected' WHERE id='${channel.id}'`,
    );
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      brand.id,
    );
    await page.goto("/#drafts");
    await page
      .getByRole("button", { name: "Create post", exact: true })
      .click();
    const dialog = page.getByRole("dialog");
    await dialog.getByRole("button", { name: "Local location" }).click();
    await dialog.getByRole("tab", { name: "Local location" }).click();
    await dialog.getByLabel("Working title").fill("Internal fixture title");
    await dialog.getByLabel("Local location text").fill("Isolated event");
    await dialog.getByLabel("Post type").selectOption("EVENT");
    await dialog.getByLabel("Event title").fill("Fixture event");
    await dialog.getByLabel("Event start date").fill("2027-02-11");
    await dialog.getByLabel("Event start time").fill("09:15");
    await dialog.getByLabel("Event end date").fill("2027-02-12");
    await dialog.getByLabel("Event end time").fill("17:45");
    await dialog.getByLabel("Call to action").selectOption("LEARN_MORE");
    await dialog
      .getByLabel("Call to action URL")
      .fill("https://example.test/event");
    await dialog.getByLabel("When should this go out?").selectOption("exact");
    await dialog
      .getByLabel(/Date and time ·/)
      .fill(new Date(Date.now() + 2 * 86400000).toISOString().slice(0, 16));
    await dialog.getByRole("button", { name: "Schedule post" }).click();
    await expect(dialog).toHaveCount(0);
    const response = await context.request.get(
      `/api/v1/publications?brand_id=${brand.id}`,
    );
    expect(response.ok()).toBe(true);
    const [publication] = (await response.json()).data;
    expect(publication.state).toBe("queued");
    expect(publication.payload.options).toEqual({
      topic_type: "EVENT",
      event: {
        title: "Fixture event",
        start_date: "2027-02-11",
        start_time: "09:15",
        end_date: "2027-02-12",
        end_time: "17:45",
      },
      call_to_action: { type: "LEARN_MORE", url: "https://example.test/event" },
    });
    const attempt = randomUUID(),
      checkpointId = randomUUID(),
      newerCheckpointId = randomUUID();
    const checkpoint = {
      stage: "google_post_started",
      attempt_id: attempt,
      checkpoint_id: checkpointId,
    };
    const encrypted = encryptedCheckpoint(publication.id, checkpoint);
    sql(
      `UPDATE publications SET state='uncertain' WHERE id='${publication.id}'; INSERT INTO publication_attempts(id,publication_id,state,provider,finished_at) VALUES('${attempt}','${publication.id}','uncertain','google_business',now()); INSERT INTO provider_delivery_checkpoints(publication_id,encrypted_checkpoint,attempt_id,checkpoint_id) VALUES('${publication.id}','${encrypted}','${attempt}','${checkpointId}')`,
    );
    const uncertain = await context.request.get(
      `/api/v1/publications?brand_id=${brand.id}`,
    );
    expect((await uncertain.json()).data[0].state).toBe("uncertain");
    await page.goto("/#queue");
    await page.reload();
    await page.getByRole("button", { name: "Needs attention" }).click();
    await expect(page.locator("main")).toContainText("Isolated event");
    await page.getByRole("button", { name: "Reconcile outcome" }).click();
    await expect(dialog).toContainText(attempt);
    await expect(dialog).toContainText(checkpointId);
    await expect(dialog).toContainText("provider status check is unavailable");
    await expect(
      dialog.getByRole("button", { name: "Retry publication" }),
    ).toHaveCount(0);
    await expect(
      dialog.getByRole("button", { name: "Cancel publication" }),
    ).toHaveCount(0);
    await dialog.getByLabel("Verified outcome").selectOption("not_published");
    await dialog
      .getByLabel("Verification note")
      .fill("Fixture: remote operation definitively canceled");
    await dialog
      .getByRole("checkbox", { name: /original sender has stopped/ })
      .check();
    await dialog
      .getByRole("checkbox", { name: /operation definitively failed/ })
      .check();
    const newerCheckpoint = {
      ...checkpoint,
      checkpoint_id: newerCheckpointId,
    };
    const newerEncrypted = encryptedCheckpoint(publication.id, newerCheckpoint);
    sql(
      `UPDATE provider_delivery_checkpoints SET encrypted_checkpoint='${newerEncrypted}',checkpoint_id='${newerCheckpointId}' WHERE publication_id='${publication.id}'`,
    );
    await dialog.getByRole("button", { name: "Record outcome" }).click();
    await expect(dialog).toContainText(newerCheckpointId);
    await expect(
      dialog.getByRole("button", { name: "Retry publication" }),
    ).toHaveCount(0);
    await dialog.getByLabel("Verified outcome").selectOption("not_published");
    await dialog
      .getByLabel("Verification note")
      .fill("Fixture: newer remote operation definitively canceled");
    const stopped = dialog.getByRole("checkbox", {
      name: /original sender has stopped/,
    });
    const inspected = dialog.getByRole("checkbox", {
      name: /operation definitively failed/,
    });
    await expect(stopped).not.toBeChecked();
    await expect(inspected).not.toBeChecked();
    await stopped.check();
    await inspected.check();
    await dialog.getByRole("button", { name: "Record outcome" }).click();
    await expect(
      dialog.getByRole("button", { name: "Retry publication" }),
    ).toBeEnabled();
    const status = await context.request.get(
      `/api/v1/publications/${publication.id}/provider-status`,
    );
    expect((await status.json()).data.google_delivery).toMatchObject({
      attempt_id: attempt,
      checkpoint_id: newerCheckpointId,
      retry_allowed: true,
    });
    await dialog.getByRole("button", { name: "Retry publication" }).click();
    const updated = await context.request.get(
      `/api/v1/publications?brand_id=${brand.id}`,
    );
    expect((await updated.json()).data[0].state).toBe("queued");
  });
});
