import { useEffect, useState } from "react";
import { z } from "zod";
import { api, type Publication } from "../api";
import { ErrorNotice } from "../components/shared";
import { Button } from "../components/ui/button";
const Status = z.object({
  session_expired: z.boolean(),
  reset_allowed: z.boolean(),
  reason: z.string().nullable(),
});
export function ProviderRecovery({
  publication,
  action,
  pending,
  onRetryAllowed,
}: {
  publication: Publication;
  action: (fn: () => Promise<unknown>, message?: string) => Promise<boolean>;
  pending: boolean;
  onRetryAllowed: (allowed: boolean) => void;
}) {
  const [status, setStatus] = useState<z.infer<typeof Status> | null>(null);
  const [error, setError] = useState("");
  const [note, setNote] = useState("");
  const [confirm, setConfirm] = useState(false);
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    let active = true;
    onRetryAllowed(false);
    api(`/publications/${publication.id}/provider-status`, Status)
      .then((result) => {
        if (active) {
          setStatus(result);
          onRetryAllowed(!result.session_expired);
          setError("");
        }
      })
      .catch((e) => {
        if (active) setError(e.message);
      });
    return () => {
      active = false;
    };
  }, [publication.id, publication.updated_at, revision, onRetryAllowed]);
  return (
    <section className="detail-section form-stack">
      <h3>Provider recovery</h3>
      {error && <ErrorNotice message={error} />}
      {status?.reason && <p>{status.reason}</p>}
      <Button
        variant="secondary"
        disabled={pending}
        onClick={() =>
          void action(async () => {
            const result = await api(
              `/publications/${publication.id}/provider-reconcile`,
              z.object({ state: z.string(), reason: z.string().optional() }),
              "POST",
              {},
            );
            setNote(
              result.reason ??
                `Provider reports: ${result.state.replaceAll("_", " ")}`,
            );
            setRevision((value) => value + 1);
          }, "Provider outcome checked")
        }
      >
        Check provider outcome
      </Button>
      {note && <p role="status">{note}</p>}
      {status?.session_expired && (
        <p>
          The upload session expired. This does not prove the video was not
          published. Inspect the network, then record a verified not-published
          reconciliation before resetting.
        </p>
      )}
      {status?.reset_allowed && (
        <>
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={confirm}
              onChange={(event) => setConfirm(event.target.checked)}
            />
            I verified that this video was not published and recorded that
            reconciliation. Reset this expired upload session.
          </label>
          <Button
            variant="destructive"
            disabled={pending || !confirm}
            onClick={() =>
              void action(async () => {
                const result = await api(
                  `/publications/${publication.id}/reset-expired-session`,
                  z.object({
                    state: z.literal("failed"),
                    session_reset: z.literal(true),
                    next_action: z.string(),
                  }),
                  "POST",
                  {},
                );
                setNote(result.next_action);
                setConfirm(false);
                setRevision((value) => value + 1);
              }, "Expired session reset; publication has not been retried")
            }
          >
            Reset expired upload session
          </Button>
        </>
      )}
    </section>
  );
}
