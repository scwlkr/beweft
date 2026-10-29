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
  GoogleDeliveryStatus,
  ReconcileRequest,
} from "../lib/api-contract.generated";
import { ErrorNotice, Field } from "../components/shared";
import { Button } from "../components/ui/button";

const Checkpoint: z.ZodType<GoogleDeliveryStatus> = z.object({
  phase: z.enum(["create_started", "processing", "rejected"]),
  attempt_id: z.string().uuid(),
  checkpoint_id: z.string().uuid(),
  post_name: z.string().nullable(),
  retry_allowed: z.boolean(),
  reason: z.string(),
});
// An older server may omit this field; omission is not a safe retry signal.
const Status = z.object({ google_delivery: Checkpoint.nullable().optional() });
type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;

export function GoogleRecovery({
  publication,
  action,
  pending,
  onRetryAllowed,
}: {
  publication: Publication;
  action: Action;
  pending: boolean;
  onRetryAllowed: (allowed: boolean) => void;
}) {
  const [status, setStatus] = useState<z.infer<typeof Status>>();
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [revision, setRevision] = useState(0);
  const [resolution, setResolution] = useState<
    "" | "published" | "not_published"
  >("");
  const [note, setNote] = useState("");
  const [url, setUrl] = useState("");
  const [stopped, setStopped] = useState(false);
  const [inspected, setInspected] = useState(false);
  const [busy, setBusy] = useState(false);
  const alive = useRef(false);
  const running = useRef(false);

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
    setStopped(false);
    setInspected(false);
    onRetryAllowed(false);
    api(
      `/publications/${publication.id}/provider-status`,
      Status,
      "GET",
      undefined,
      controller.signal,
    )
      .then((value) => {
        if (!active || !isCurrentWorkspaceSession(session)) return;
        setStatus(value);
        onRetryAllowed(value.google_delivery?.retry_allowed === true);
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
  }, [publication.id, publication.updated_at, revision, onRetryAllowed]);

  const checkpoint = status?.google_delivery;
  const canResolve =
    !!checkpoint &&
    (publication.state === "uncertain" ||
      (publication.state === "failed" && !checkpoint.retry_allowed));
  const resolve = async () => {
    if (
      running.current ||
      pending ||
      !checkpoint ||
      !resolution ||
      !note.trim() ||
      !stopped ||
      !inspected
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
      ...(resolution === "published" && url ? { external_url: url } : {}),
      observed_attempt_id: checkpoint.attempt_id,
      observed_checkpoint_id: checkpoint.checkpoint_id,
      sender_stopped: true,
      provider_inspected: true,
    };
    running.current = true;
    setBusy(true);
    setStatus(undefined);
    onRetryAllowed(false);
    try {
      const done = await action(
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
        done
          ? "Owner attestation recorded. This is not provider API confirmation."
          : "The outcome was not confirmed. Check the current status before deciding what to do next.",
      );
      setRevision((value) => value + 1);
    } finally {
      running.current = false;
      if (alive.current && isCurrentWorkspaceSession(session)) setBusy(false);
    }
  };
  const checkProvider = async () => {
    if (
      running.current ||
      pending ||
      !checkpoint?.post_name ||
      publication.state !== "uncertain"
    )
      return;
    let session;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    running.current = true;
    setBusy(true);
    setStatus(undefined);
    onRetryAllowed(false);
    let result: { state: string; reason?: string } | undefined;
    try {
      const done = await action(async () => {
        result = await api(
          `/publications/${publication.id}/provider-reconcile`,
          z.object({ state: z.string(), reason: z.string().optional() }),
          "POST",
          {},
        );
      }, "Provider outcome checked");
      if (!alive.current || !isCurrentWorkspaceSession(session)) return;
      setNotice(
        done && result
          ? (result.reason ??
              `Provider reports: ${result.state.replaceAll("_", " ")}`)
          : "The provider check was not confirmed. Refresh status before deciding what to do next.",
      );
      setRevision((value) => value + 1);
    } finally {
      running.current = false;
      if (alive.current && isCurrentWorkspaceSession(session)) setBusy(false);
    }
  };

  return (
    <section className="detail-section form-stack">
      <h3>Google Business Profile delivery recovery</h3>
      {error && <ErrorNotice message={error} />}
      {!status && !error && <p>Checking delivery status…</p>}
      {status && !checkpoint && (
        <p>
          No current Google delivery checkpoint is available. Refresh the
          status; owner recovery and retry stay unavailable until the server
          provides a safe recovery signal.
        </p>
      )}
      {checkpoint && (
        <div className="notice">
          <div>
            <strong>
              {checkpoint.phase === "rejected"
                ? "Google rejected this post"
                : checkpoint.phase === "processing"
                  ? "Google may still be processing this post"
                  : "A Google post request may have started"}
            </strong>
            <p>{checkpoint.reason}</p>
            <p>
              An accepted request can still publish after the local sender
              stops. An absent post, a read error, or a 404 alone does not prove
              that the remote operation has ended.
            </p>
            <p>
              Attempt: <code>{checkpoint.attempt_id}</code>
              <br />
              Checkpoint: <code>{checkpoint.checkpoint_id}</code>
              {checkpoint.post_name && (
                <>
                  <br />
                  Google post: <code>{checkpoint.post_name}</code>
                </>
              )}
            </p>
          </div>
        </div>
      )}
      <div className="publication-actions">
        <Button
          variant="secondary"
          disabled={busy || pending}
          onClick={() => setRevision((value) => value + 1)}
        >
          Check delivery status
        </Button>
        {publication.state === "uncertain" && checkpoint?.post_name && (
          <Button
            variant="secondary"
            disabled={busy || pending}
            onClick={() => void checkProvider()}
          >
            Check provider outcome
          </Button>
        )}
      </div>
      {checkpoint &&
        publication.state === "uncertain" &&
        !checkpoint.post_name && (
          <p className="muted">
            Google did not return a saved post name, so a provider status check
            is unavailable. Inspect the location before recording an owner
            outcome.
          </p>
        )}
      {notice && <p role="status">{notice}</p>}
      {canResolve && (
        <form
          className="form-stack"
          onSubmit={(event) => {
            event.preventDefault();
            void resolve();
          }}
        >
          <p>
            Inspect Google before recording an outcome. Your attestation does
            not send a post and is not provider confirmation.
          </p>
          <Field label="Verified outcome">
            <select
              required
              value={resolution}
              onChange={(event) => {
                setResolution(event.target.value as typeof resolution);
                setStopped(false);
                setInspected(false);
              }}
            >
              <option value="">Choose verified outcome</option>
              {publication.state === "uncertain" && (
                <option value="published">The post exists on Google</option>
              )}
              <option value="not_published">
                Not published; the Google operation definitively ended
              </option>
            </select>
          </Field>
          <Field label="Verification note">
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
          {resolution && (
            <>
              <label className="checkbox-label">
                <input
                  type="checkbox"
                  checked={stopped}
                  onChange={(event) => setStopped(event.target.checked)}
                />
                I confirmed the original sender has stopped.
              </label>
              <label className="checkbox-label">
                <input
                  type="checkbox"
                  checked={inspected}
                  onChange={(event) => setInspected(event.target.checked)}
                />
                {resolution === "not_published"
                  ? "I confirmed Google's operation definitively failed, stopped, or was canceled. An absent post or stopped local sender alone does not prove it cannot publish later."
                  : "I inspected Google and confirmed this post exists."}
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
              !stopped ||
              !inspected
            }
          >
            Record outcome
          </Button>
        </form>
      )}
      {checkpoint?.retry_allowed && (
        <p className="muted">
          Retry is a separate action and creates a new Google post.
        </p>
      )}
    </section>
  );
}
