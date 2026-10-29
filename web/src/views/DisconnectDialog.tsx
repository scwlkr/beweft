import { useEffect, useRef, useState } from "react";
import { z } from "zod";
import {
  api,
  captureWorkspaceSession,
  isCurrentWorkspaceSession,
  type Channel,
} from "../api";
import type { DisconnectRequest } from "../lib/api-contract.generated";
import { ErrorNotice } from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";

const DisconnectResult = z.object({
  disconnected: z.literal(true),
  provider_revoked: z.literal(false),
  channels_requiring_reconnection: z.number().int().nonnegative(),
  revocation_id: z.string().uuid().nullable(),
  next_action: z.string(),
});

type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;

export function DisconnectDialog({
  channel,
  pending,
  action,
  onClose,
  onReview,
}: {
  channel: Channel;
  pending: boolean;
  action: Action;
  onClose: () => void;
  onReview: () => void;
}) {
  const googleFamily = ["youtube", "google_business"].includes(
    channel.provider,
  );
  const [confirmed, setConfirmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<z.infer<typeof DisconnectResult> | null>(
    null,
  );
  const [error, setError] = useState("");
  const running = useRef(false);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const disconnect = async () => {
    if (!confirmed || pending || running.current || result || error) return;
    let session;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    running.current = true;
    setBusy(true);
    let response: z.infer<typeof DisconnectResult> | null = null;
    let failure: unknown = null;
    try {
      await action(async () => {
        try {
          response = await api(
            `/channels/${channel.id}/disconnect`,
            DisconnectResult,
            "POST",
            { confirm_shared_impact: true } satisfies DisconnectRequest,
          );
        } catch (caught) {
          failure = caught;
          throw caught;
        }
      }, "Channel disconnected");
      if (!alive.current || !isCurrentWorkspaceSession(session)) return;
      if (response) setResult(response);
      else if (failure) setError((failure as Error).message);
    } finally {
      running.current = false;
      if (alive.current && isCurrentWorkspaceSession(session)) setBusy(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => !open && !busy && onClose()}
      title={`Disconnect ${channel.name}?`}
      description={
        googleFamily
          ? "This removes the selected channel’s stored credentials and cancels its unsent queued deliveries. Publishing history remains."
          : "This removes the selected channel’s stored credentials and cancels its queued deliveries. Publishing history remains."
      }
    >
      <div className="form-stack">
        {googleFamily ? (
          <p>
            YouTube and Google Business Profile may share Google authorization
            across current and prior Google projects and accounts. Other
            affected beweft Google channels, including those in other brands,
            keep their stored credentials and queues but are marked for
            reconnection. Inspect those channels before removing the relevant
            beweft access at Google. beweft does not revoke access at Google.
          </p>
        ) : (
          <p>
            Other channels for this provider, including accounts in other
            brands, may need to reconnect if access is removed. beweft does not
            revoke access at the provider.
          </p>
        )}
        {!result && !error && (
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={confirmed}
              onChange={(event) => setConfirmed(event.target.checked)}
            />
            I understand the selected queue will be canceled and other accounts
            may require reconnection.
          </label>
        )}
        {result && (
          <div className="notice" role="status">
            <div>
              <strong>
                Channel disconnected; provider access has not been removed by
                beweft.
              </strong>
              <p>{result.next_action}</p>
              {result.channels_requiring_reconnection > 0 && (
                <p>
                  {result.channels_requiring_reconnection} other channel(s) were
                  marked for reconnection.
                </p>
              )}
            </div>
          </div>
        )}
        {error && (
          <>
            <ErrorNotice message={error} />
            <p>
              The disconnect outcome may be unknown. Refresh the workspace and
              check provider removal status before taking further action.
            </p>
          </>
        )}
        {!result && !error && (
          <Button
            variant="destructive"
            disabled={pending || busy || !confirmed}
            onClick={() => void disconnect()}
          >
            {busy ? "Disconnecting…" : "Disconnect channel"}
          </Button>
        )}
        {(result || error) && (
          <Button variant="secondary" onClick={onReview}>
            Review provider removal status
          </Button>
        )}
      </div>
    </Dialog>
  );
}
