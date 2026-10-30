import { useEffect, useRef, useState } from "react";
import { z } from "zod";
import {
  api,
  captureWorkspaceSession,
  isCurrentWorkspaceSession,
  PublicationSchema,
  type Publication,
} from "../api";
import type {
  ReconcileRequest,
  ResetXUploadRequest,
  XThreadStatus,
  XUploadStatus,
} from "../lib/api-contract.generated";
import { ErrorNotice, Field } from "../components/shared";
import { Button } from "../components/ui/button";
import { XThreadRecovery } from "./XThreadRecovery";

const XCheckpoint: z.ZodType<XUploadStatus> = z.object({
  phase: z.enum(["upload", "post_started"]),
  attempt_id: z.string().uuid(),
  checkpoint_id: z.string().uuid(),
  reset_allowed: z.boolean(),
  reason: z.string(),
});
const ThreadCheckpoint: z.ZodType<XThreadStatus> = z
  .object({
    phase: z.enum(["preparing", "ready", "request_started", "complete"]),
    confirmed_ids: z.array(z.string().regex(/^\d{1,19}$/)),
    next_index: z.number().int().min(0),
    total: z.number().int().min(2).max(10),
    remaining_texts: z.array(z.string()),
    attempt_id: z.string().uuid(),
    checkpoint_id: z.string().uuid(),
    root_request_started: z.boolean().default(false),
  })
  .refine(
    (value) =>
      value.next_index === value.confirmed_ids.length &&
      value.next_index <= value.total &&
      value.remaining_texts.length === value.total - value.next_index &&
      new Set(value.confirmed_ids).size === value.confirmed_ids.length,
  );
const Status = z.object({
  x_upload: XCheckpoint.nullable().optional(),
  x_thread: ThreadCheckpoint.nullable().optional(),
  x_thread_retry_allowed: z.boolean().optional(),
});
const ResetResult = z.object({
  state: z.literal("failed"),
  session_reset: z.literal(true),
  next_action: z.string(),
});
type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;

export function XRecovery({
  publication,
  action,
  pending,
  onRetryAllowed,
  onCancelAllowed,
}: {
  publication: Publication;
  action: Action;
  pending: boolean;
  onRetryAllowed: (allowed: boolean) => void;
  onCancelAllowed: (allowed: boolean) => void;
}) {
  const [status, setStatus] = useState<z.infer<typeof Status> | undefined>();
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [refresh, setRefresh] = useState(0);
  const [resolution, setResolution] = useState<
    "" | "published" | "not_published"
  >("");
  const [note, setNote] = useState("");
  const [url, setUrl] = useState("");
  const [senderStopped, setSenderStopped] = useState(false);
  const [inspected, setInspected] = useState(false);
  const [restart, setRestart] = useState(false);
  const [busy, setBusy] = useState(false);
  const alive = useRef(false);
  const running = useRef(false);
  const threadIntent = publication.payload.options?.x_thread !== undefined;

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  useEffect(() => {
    let active = true;
    const controller = new AbortController();
    let session;
    try {
      session = captureWorkspaceSession();
    } catch {
      return () => controller.abort();
    }
    setStatus(undefined);
    setError("");
    setResolution("");
    setNote("");
    setUrl("");
    setSenderStopped(false);
    setInspected(false);
    setRestart(false);
    onRetryAllowed(false);
    onCancelAllowed(false);
    api(
      `/publications/${publication.id}/provider-status`,
      Status,
      "GET",
      undefined,
      controller.signal,
    )
      .then((result) => {
        if (!active || !isCurrentWorkspaceSession(session)) return;
        setStatus(result);
        onRetryAllowed(
          threadIntent
            ? result.x_upload === null && result.x_thread_retry_allowed === true
            : result.x_upload === null,
        );
        onCancelAllowed(
          !threadIntent ||
            result.x_thread?.phase === "preparing" ||
            (result.x_upload === null &&
              result.x_thread_retry_allowed === true),
        );
      })
      .catch((failure) => {
        if (
          active &&
          !controller.signal.aborted &&
          isCurrentWorkspaceSession(session)
        )
          setError((failure as Error).message);
      });
    return () => {
      active = false;
      controller.abort();
    };
  }, [
    publication.id,
    publication.updated_at,
    refresh,
    onRetryAllowed,
    onCancelAllowed,
    threadIntent,
  ]);

  const checkpoint = status?.x_upload;
  const observed = status !== undefined && checkpoint !== undefined;
  const publicBoundary = checkpoint?.phase === "post_started";
  const requiresReview = publicBoundary && resolution === "not_published";
  const canResolve =
    publication.state === "uncertain" ||
    (publication.state === "failed" &&
      publicBoundary &&
      !checkpoint.reset_allowed);
  const resolve = async () => {
    if (
      running.current ||
      pending ||
      !resolution ||
      !note.trim() ||
      (resolution === "not_published" && !observed) ||
      (requiresReview && (!senderStopped || !inspected))
    )
      return;
    let session;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    const body: ReconcileRequest = {
      resolution,
      note,
      ...(url ? { external_url: url } : {}),
      ...(requiresReview && checkpoint
        ? {
            observed_attempt_id: checkpoint.attempt_id,
            observed_checkpoint_id: checkpoint.checkpoint_id,
            sender_stopped: true,
            provider_inspected: true,
          }
        : {}),
    };
    running.current = true;
    setBusy(true);
    setStatus(undefined);
    onRetryAllowed(false);
    try {
      const ok = await action(
        () =>
          api(
            `/publications/${publication.id}/reconcile`,
            PublicationSchema,
            "POST",
            body,
          ),
        "Owner outcome recorded",
      );
      if (!alive.current || !isCurrentWorkspaceSession(session)) return;
      setNotice(
        ok
          ? "Owner attestation recorded. It is not provider API confirmation."
          : "The outcome was not confirmed. Review the refreshed status before deciding what to do next.",
      );
      setRefresh((value) => value + 1);
    } finally {
      running.current = false;
      if (alive.current && isCurrentWorkspaceSession(session)) setBusy(false);
    }
  };
  const reset = async () => {
    if (
      !checkpoint ||
      !checkpoint.reset_allowed ||
      !senderStopped ||
      !restart ||
      (publicBoundary && !inspected) ||
      running.current ||
      pending
    )
      return;
    let session;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    const body: ResetXUploadRequest = {
      observed_attempt_id: checkpoint.attempt_id,
      observed_checkpoint_id: checkpoint.checkpoint_id,
      sender_stopped: true,
      confirm_restart: true,
      ...(publicBoundary ? { provider_inspected: true } : {}),
    };
    running.current = true;
    setBusy(true);
    setStatus(undefined);
    onRetryAllowed(false);
    const result: { value: z.infer<typeof ResetResult> | null } = {
      value: null,
    };
    try {
      const ok = await action(async () => {
        result.value = await api(
          `/publications/${publication.id}/reset-x-upload`,
          ResetResult,
          "POST",
          body,
        );
      }, "X checkpoint reset; publication has not been retried");
      if (!alive.current || !isCurrentWorkspaceSession(session)) return;
      setNotice(
        result.value
          ? result.value.next_action
          : ok
            ? "Checkpoint reset. Retry remains a separate action."
            : "The reset outcome was not confirmed. Review the refreshed status before deciding what to do next.",
      );
      setRefresh((value) => value + 1);
    } finally {
      running.current = false;
      if (alive.current && isCurrentWorkspaceSession(session)) setBusy(false);
    }
  };

  return (
    <section className="detail-section form-stack">
      <h3>X delivery recovery</h3>
      {error && <ErrorNotice message={error} />}
      {status && checkpoint === undefined && (
        <p>
          This server cannot report X checkpoints. Update the server before
          recording a not-published outcome or retrying.
        </p>
      )}
      {threadIntent && status && status.x_thread === undefined && (
        <p>
          This server cannot report X thread progress. Update the server before
          recovery.
        </p>
      )}
      {threadIntent &&
        status &&
        status.x_thread === null &&
        !status.x_thread_retry_allowed && (
          <p>
            No usable X thread checkpoint is recorded. Inspect the actual chain;
            a missing checkpoint cannot prove that nothing was sent.
          </p>
        )}
      {!status && !error && <p>Checking X recovery status…</p>}
      {error && (
        <p>
          X checkpoint status is unknown. Check it again before recording a
          not-published outcome or retrying.
        </p>
      )}
      {checkpoint && (
        <div className="notice">
          <div>
            <strong>
              {checkpoint.phase === "upload"
                ? "Upload in progress or interrupted"
                : "A post request may have started"}
            </strong>
            <p>{checkpoint.reason}</p>
            <p>
              Attempt: <code>{checkpoint.attempt_id}</code>
              <br />
              Checkpoint: <code>{checkpoint.checkpoint_id}</code>
            </p>
            <p>
              An owner attestation does not prove a request accepted by X was
              canceled.
            </p>
          </div>
        </div>
      )}
      {status && checkpoint === null && (
        <p>No X checkpoint is recorded for this publication.</p>
      )}
      <Button
        type="button"
        variant="secondary"
        disabled={busy || pending}
        onClick={() => setRefresh((value) => value + 1)}
      >
        Check X recovery status
      </Button>
      {notice && <p role="status">{notice}</p>}
      {threadIntent && status?.x_thread && (
        <XThreadRecovery
          key={`${status.x_thread.attempt_id}:${status.x_thread.checkpoint_id}`}
          publicationId={publication.id}
          thread={status.x_thread}
          action={action}
          pending={pending}
          onChanged={() => setRefresh((value) => value + 1)}
        />
      )}
      {!threadIntent && canResolve && (
        <form
          className="form-stack"
          onSubmit={(event) => {
            event.preventDefault();
            void resolve();
          }}
        >
          <p>
            Inspect X before recording an outcome. This is your attestation and
            does not send a post.
          </p>
          <Field label="Verified X outcome">
            <select
              required
              value={resolution}
              onChange={(event) => {
                setResolution(
                  event.target.value as "" | "published" | "not_published",
                );
                setSenderStopped(false);
                setInspected(false);
              }}
            >
              <option value="">Choose the outcome you verified</option>
              {publication.state === "uncertain" && (
                <option value="published">The post exists on X</option>
              )}
              {observed && (
                <option value="not_published">
                  I verified it was not published
                </option>
              )}
            </select>
          </Field>
          <Field label="X verification note">
            <textarea
              required
              rows={2}
              value={note}
              onChange={(event) => setNote(event.target.value)}
            />
          </Field>
          {resolution === "published" && (
            <Field label="Published post URL (optional)">
              <input
                type="url"
                value={url}
                onChange={(event) => setUrl(event.target.value)}
              />
            </Field>
          )}
          {requiresReview && (
            <>
              <label className="checkbox-label">
                <input
                  type="checkbox"
                  checked={senderStopped}
                  onChange={(event) => setSenderStopped(event.target.checked)}
                />
                I confirmed the original sender has stopped.
              </label>
              <label className="checkbox-label">
                <input
                  type="checkbox"
                  checked={inspected}
                  onChange={(event) => setInspected(event.target.checked)}
                />
                I inspected X and verified this attempt was not published.
              </label>
            </>
          )}
          <Button
            type="submit"
            variant="secondary"
            disabled={
              busy ||
              pending ||
              !resolution ||
              !note.trim() ||
              (requiresReview && (!senderStopped || !inspected))
            }
          >
            Record X outcome
          </Button>
        </form>
      )}
      {checkpoint?.reset_allowed && (
        <div className="form-stack">
          <p>
            Reset clears this checkpoint. It does not queue a post; retry is a
            separate action.
          </p>
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={senderStopped}
              onChange={(event) => setSenderStopped(event.target.checked)}
            />
            I confirmed the original sender has stopped.
          </label>
          {publicBoundary && (
            <label className="checkbox-label">
              <input
                type="checkbox"
                checked={inspected}
                onChange={(event) => setInspected(event.target.checked)}
              />
              {threadIntent
                ? "I inspected X and confirmed the root operation definitively ended without creating a post."
                : "I inspected X for this attempt before resetting."}
            </label>
          )}
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={restart}
              onChange={(event) => setRestart(event.target.checked)}
            />
            I want to reset this checkpoint so I can decide whether to retry.
          </label>
          <Button
            type="button"
            variant="destructive"
            disabled={
              busy ||
              pending ||
              !senderStopped ||
              !restart ||
              (publicBoundary && !inspected)
            }
            onClick={() => void reset()}
          >
            Reset X checkpoint
          </Button>
        </div>
      )}
    </section>
  );
}
