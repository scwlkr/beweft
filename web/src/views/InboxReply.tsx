import { Send } from "lucide-react";
import { z } from "zod";
import { ReplyResolution } from "./ReplyResolution";
import { useReply } from "./useReply";
import { ErrorNotice, Field } from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";
import { ItemSchema } from "./insight-contracts";
export function Reply({
  item,
  brandId,
  onClose,
  onSent,
}: {
  item: z.infer<typeof ItemSchema>;
  brandId: string;
  onClose: () => void;
  onSent: () => Promise<void>;
}) {
  const {
    text,
    setText,
    attempt,
    pending,
    busy,
    ready,
    error,
    check,
    send,
    another,
    resolve,
    historyChanged,
    latest,
    acknowledgeHistory,
    locked,
  } = useReply(brandId, item);
  const unresolved =
    attempt && ["uncertain", "sending"].includes(attempt.state);
  const terminal = attempt && ["published", "failed"].includes(attempt.state);
  return (
    <Dialog
      open
      onOpenChange={(v) => !v && !busy && onClose()}
      title={`Reply to ${item.item.author || "this conversation"}`}
      description={`This sends a real reply through ${item.channel_name}.`}
    >
      <form
        className="form-stack"
        onSubmit={(event) => {
          event.preventDefault();
          void send();
        }}
      >
        <p className="detail-copy post-copy">{item.item.text}</p>
        {item.provider === "mastodon" && item.item.reply_context && (
          <div className="notice">
            <div>
              <strong>
                Original audience: {item.item.reply_context.visibility}
              </strong>
              <p>
                Participants:{" "}
                {item.item.reply_context.recipients
                  .map((recipient) => `@${recipient.acct}`)
                  .join(", ") || "No reported recipients"}
              </p>
              <p>
                {["private", "direct"].includes(
                  item.item.reply_context.visibility,
                )
                  ? "Your reply is sent directly to the listed original participants, not to your followers."
                  : "Your reply keeps the original public or unlisted visibility."}{" "}
                The server adds the original participants. Do not add @
                mentions; expanding the recipient list is blocked.
              </p>
            </div>
          </div>
        )}
        <Field label="Your reply">
          <textarea
            required
            disabled={locked}
            rows={5}
            value={text}
            onChange={(e) => setText(e.target.value)}
          />
        </Field>
        {error && <ErrorNotice message={error} />}
        {attempt && (
          <div className="notice" role="status">
            <div>
              <strong>Server reply outcome: {attempt.state}</strong>
              <p>Request {attempt.id}</p>
              {attempt.receipt &&
              typeof attempt.receipt === "object" &&
              "verification" in attempt.receipt &&
              attempt.receipt.verification === "owner_attestation" ? (
                <p>
                  Owner attestation: this outcome was recorded after the owner
                  checked the network; it is not provider API confirmation.
                </p>
              ) : null}
              {unresolved && (
                <p>
                  The server has an unresolved attempt. New replies are blocked;
                  inspect the network and check this attempt again.
                </p>
              )}
            </div>
          </div>
        )}
        {attempt?.state === "uncertain" && (
          <ReplyResolution disabled={busy || !ready} onResolve={resolve} />
        )}
        {!attempt && pending && (
          <p role="status">
            The original request is remembered on this device, but no durable
            server outcome is available yet. Checking never sends a reply.
          </p>
        )}
        {historyChanged && pending && (
          <section className="notice">
            <div>
              <strong>Another reply changed this conversation</strong>
              {latest && (
                <p>
                  Latest server reply: {latest.state} · {latest.text}
                </p>
              )}
              <p>
                Your original intent is still preserved. Review the newer
                outcome before choosing whether that original reply is still
                needed.
              </p>
              <Button
                type="button"
                variant="secondary"
                disabled={busy || !ready || !!unresolved}
                onClick={() => void acknowledgeHistory()}
              >
                I reviewed the newer outcome; keep my original intent
              </Button>
            </div>
          </section>
        )}
        <div className="inline-actions">
          <Button
            type="button"
            variant="secondary"
            disabled={busy}
            onClick={() => void check()}
          >
            {busy ? "Checking…" : "Check existing reply outcome"}
          </Button>
          {terminal ? (
            <>
              <Button
                type="button"
                disabled={
                  busy ||
                  !ready ||
                  (!!pending && pending.request_id !== attempt.id)
                }
                onClick={another}
              >
                Write another reply
              </Button>
              <Button
                type="button"
                variant="ghost"
                disabled={busy}
                onClick={() => void onSent().then(onClose)}
              >
                Done
              </Button>
            </>
          ) : (
            !unresolved && (
              <Button
                disabled={busy || !ready || historyChanged || !text.trim()}
              >
                <Send size={16} />
                {pending ? "Retry original reply" : "Send reply"}
              </Button>
            )
          )}
        </div>
      </form>
    </Dialog>
  );
}
