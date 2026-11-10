import { useEffect, useRef, useState } from "react";
import { z } from "zod";
import {
  api,
  PublicationSchema,
  type Publication,
} from "../api";
import { ErrorNotice } from "../components/shared";
import { Button } from "../components/ui/button";

type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;

export function BlueskyMigrate({
  channelId,
  channelName,
  action,
  pending,
  onConfigureOrigin,
  onConnectNewChannel,
  discoveredOrigin,
  onClose,
}: {
  channelId?: string;
  channelName?: string;
  action: Action;
  pending: boolean;
  onConfigureOrigin?: () => void;
  onConnectNewChannel?: () => void;
  discoveredOrigin?: string;
  onClose?: () => void;
}) {
  const [queued, setQueued] = useState<Publication[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [decision, setDecision] = useState<"preserve" | "retire" | null>(null);
  const alive = useRef(true);

  const loadQueued = async () => {
    if (!channelId) return;
    setLoading(true);
    setError("");
    try {
      const rows = await api(
        `/publications?channel_id=${encodeURIComponent(channelId)}&group=queue`,
        z.array(PublicationSchema),
      );
      if (alive.current) setQueued(rows);
    } catch (failure) {
      if (alive.current) setError((failure as Error).message);
    } finally {
      if (alive.current) setLoading(false);
    }
  };

  useEffect(() => {
    alive.current = true;
    void loadQueued();
    return () => {
      alive.current = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [channelId]);

  const preserve = async () => {
    if (busy || pending) return;
    const ok = await action(async () => {}, "Queues preserved on the old channel");
    if (ok && alive.current) {
      setDecision("preserve");
      setMessage(
        "Queues are preserved on the old channel. Grants and tokens are not moved automatically.",
      );
    }
  };

  const retire = async () => {
    if (!confirm || busy || pending) return;
    setBusy(true);
    setError("");
    setMessage("");
    let failed = 0;
    try {
      const ok = await action(async () => {
        for (const publication of queued) {
          try {
            await api(`/publications/${publication.id}/cancel`, PublicationSchema, "POST", {});
          } catch {
            failed += 1;
          }
        }
        if (failed > 0) throw new Error(`${failed} publication(s) could not be canceled.`);
      }, "Queued publications retired");
      if (alive.current) {
        if (ok) {
          setDecision("retire");
          setMessage("Queued publications were canceled on the old channel.");
          setConfirm(false);
          await loadQueued();
        } else if (failed > 0) {
          setError(`${failed} publication(s) could not be canceled. Review and retry.`);
        }
      }
    } finally {
      if (alive.current) setBusy(false);
    }
  };

  return (
    <section className="detail-section form-stack" aria-label="Bluesky PDS migration">
      <h3>Bluesky PDS migration</h3>
      <p className="muted">
        Grants and tokens are not moved automatically. Live qualification is unverified.
      </p>
      {discoveredOrigin && (
        <p>
          Discovered PDS origin: <code>{discoveredOrigin}</code>
        </p>
      )}
      {channelName && (
        <p>
          Old channel: <strong>{channelName}</strong>. Its queues remain there until you choose otherwise.
        </p>
      )}
      <div className="form-stack">
        <h4>1. Configure the discovered PDS origin</h4>
        <p>
          Set the Bluesky instance URL to the discovered origin, save the provider app, and approve that origin on your server.
        </p>
        {onConfigureOrigin && (
          <Button type="button" variant="secondary" disabled={pending || busy} onClick={onConfigureOrigin}>
            Configure PDS origin
          </Button>
        )}
      </div>
      <div className="form-stack">
        <h4>2. Connect a new channel at that origin</h4>
        <p>
          Connect a new Bluesky channel there. Reconnect the old channel only when it is an intentional same-origin reconnect.
        </p>
        {onConnectNewChannel && (
          <Button type="button" variant="secondary" disabled={pending || busy} onClick={onConnectNewChannel}>
            Connect new channel
          </Button>
        )}
      </div>
      <div className="form-stack">
        <h4>3. Preserve or retire queues on the old channel</h4>
        {loading && <p>Checking queued publications…</p>}
        {error && <ErrorNotice message={error} />}
        {!loading && !error && (
          <p>
            {queued.length === 0
              ? "No queued publications are currently scheduled on this channel."
              : `${queued.length} queued publication${queued.length === 1 ? "" : "s"} remain on this channel.`}
          </p>
        )}
        {queued.length > 0 && (
          <ul>
            {queued.map((publication) => (
              <li key={publication.id}>
                {publication.payload.title || "Untitled"}{" "}
                <span className="muted">
                  ({new Date(publication.scheduled_at).toLocaleString()})
                </span>
              </li>
            ))}
          </ul>
        )}
        {queued.length > 0 && (
          <>
            <label className="checkbox-label">
              <input
                type="checkbox"
                checked={confirm}
                onChange={(event) => setConfirm(event.target.checked)}
              />
              I understand queued publications on the old channel will be canceled if I retire them.
            </label>
            <div className="publication-actions">
              <Button
                type="button"
                variant="secondary"
                disabled={pending || busy || queued.length === 0}
                onClick={() => void preserve()}
              >
                Preserve queues
              </Button>
              <Button
                type="button"
                variant="destructive"
                disabled={pending || busy || !confirm || queued.length === 0}
                onClick={() => void retire()}
              >
                {busy ? "Retiring…" : "Retire queued publications"}
              </Button>
            </div>
          </>
        )}
        {queued.length === 0 && (
          <Button
            type="button"
            variant="secondary"
            disabled={pending || busy}
            onClick={() => void preserve()}
          >
            Preserve queues
          </Button>
        )}
        {message && <p role="status">{message}</p>}
        {decision && (
          <p className="muted">
            Decision recorded: {decision === "preserve" ? "preserve queues" : "retire queues"}.
          </p>
        )}
      </div>
      {onClose && (
        <Button type="button" variant="ghost" onClick={onClose}>
          Close
        </Button>
      )}
    </section>
  );
}
