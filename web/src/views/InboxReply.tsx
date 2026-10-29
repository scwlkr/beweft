import { Send } from "lucide-react";
import { useState } from "react";
import { z } from "zod";
import { api } from "../api";
import { ErrorNotice, Field } from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";
import { ItemSchema } from "./insight-contracts";
export function Reply({
  item,
  onClose,
  onSent,
}: {
  item: z.infer<typeof ItemSchema>;
  onClose: () => void;
  onSent: () => Promise<void>;
}) {
  const [text, setText] = useState("");
  const [outcome, setOutcome] = useState("");
  const [requestId, setRequestId] = useState(crypto.randomUUID());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  return (
    <Dialog
      open
      onOpenChange={(v) => !v && !busy && onClose()}
      title={`Reply to ${item.item.author || "this conversation"}`}
      description={`This sends a real reply through ${item.channel_name}.`}
    >
      <form
        className="form-stack"
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          setError("");
          try {
            const result = await api(
              `/inbox/${item.id}/reply`,
              z.object({
                state: z.string(),
                error: z.string().nullable().optional(),
              }),
              "POST",
              {
                text,
                request_id: requestId,
              },
            );
            setOutcome(result.state);
            if (result.state !== "published") {
              setError(
                result.error ??
                  "Reply outcome is not confirmed. Inspect the network before sending another reply.",
              );
              return;
            }
            await onSent();
            onClose();
          } catch (e) {
            setOutcome("uncertain");
            setError(
              (e as Error).message +
                " Check the existing outcome before sending another reply.",
            );
          } finally {
            setBusy(false);
          }
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
            disabled={busy || ["uncertain", "sending"].includes(outcome)}
            rows={5}
            value={text}
            onChange={(e) => {
              setText(e.target.value);
              setRequestId(crypto.randomUUID());
            }}
          />
        </Field>
        {error && <ErrorNotice message={error} />}
        <Button disabled={busy}>
          <Send size={16} />
          {busy
            ? "Sending…"
            : outcome === "uncertain" || outcome === "sending"
              ? "Check existing reply outcome"
              : "Send reply"}
        </Button>
      </form>
    </Dialog>
  );
}
