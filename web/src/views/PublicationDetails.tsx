import { Copy, ExternalLink as LinkIcon, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { z } from "zod";
import {
  api,
  PublicationSchema,
  type Channel,
  type Publication,
  type Media,
  type Post,
  type ApiSession,
} from "../api";
import {
  ErrorNotice,
  ExternalLink,
  Field,
  localDateTime,
  State,
  Submit,
  Time,
} from "../components/shared";
import { Button } from "../components/ui/button";
import { QueueReplacement } from "./QueueReplacement";
import type { PendingMedia } from "../usePendingMedia";
import type { usePendingSchedule } from "../usePendingSchedule";
import { MetaRecovery } from "./MetaRecovery";
import { ProviderRecovery } from "./ProviderRecovery";
import { XRecovery } from "./XRecovery";
import { AttemptHistory, isOwnerAttestation } from "./AttemptHistory";
import { Dialog } from "../components/ui/dialog";

type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;
const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
function safeDestination(value: string | null | undefined) {
  if (!value) return null;
  try {
    const url = new URL(value);
    return url.protocol === "https:" && !url.username && !url.password
      ? url.href
      : null;
  } catch {
    return null;
  }
}
export function PublicationDetails({
  publication: p,
  channel,
  media,
  post,
  pendingSchedule,
  pendingMedia,
  reconcileMedia,
  onEdit,
  onRefresh,
  onClose,
  action,
  pending,
}: {
  publication: Publication;
  channel?: Channel;
  media: Media[];
  post?: Post;
  pendingSchedule: ReturnType<typeof usePendingSchedule>;
  pendingMedia: PendingMedia;
  reconcileMedia: (media: Media, session: ApiSession) => void;
  onEdit: (post: Post) => void;
  onRefresh: () => Promise<void>;
  onClose: () => void;
  action: Action;
  pending: boolean;
}) {
  const guidance =
    p.provider === "manual" &&
    channel?.provider === "manual" &&
    channel.id === p.channel_id &&
    channel.brand_id === p.brand_id
      ? channel.manual_guidance
      : null;
  const destination = safeDestination(guidance?.compose_url);
  const [retryAllowed, setRetryAllowed] = useState(
    !["youtube", "x", "facebook", "instagram", "threads", "linkedin"].includes(
      p.provider,
    ),
  );
  const [copyStatus, setCopyStatus] = useState("");
  const [url, setUrl] = useState("");
  const [note, setNote] = useState("");
  const [date, setDate] = useState(localDateTime(p.scheduled_at));
  const [resolution, setResolution] = useState("");
  const [attempts, setAttempts] = useState<Record<string, unknown>[]>([]);
  const [attemptError, setAttemptError] = useState("");
  useEffect(() => {
    api(
      `/publications/${p.id}/attempts`,
      z.array(z.record(z.string(), z.unknown())),
    )
      .then(setAttempts)
      .catch((e) => setAttemptError(e.message));
  }, [p.id, p.updated_at]);
  const perform = async (path: string, body: unknown, message: string) => {
    if (
      await action(
        () =>
          api(`/publications/${p.id}/${path}`, PublicationSchema, "POST", body),
        message,
      )
    )
      onClose();
  };
  return (
    <Dialog
      open
      onOpenChange={(v) => !v && onClose()}
      title={p.payload.title || "Publication details"}
      description={`${p.channel_name} · Revision ${p.post_version}`}
    >
      <div className="form-stack">
        <State state={p.state} />
        <p className="post-copy detail-copy">{p.payload.text}</p>
        <p className="muted">
          Scheduled <Time value={p.scheduled_at} />
        </p>
        {p.last_error && <ErrorNotice message={p.last_error} />}
        <section className="detail-section">
          <h3>Outcome</h3>
          {p.external_id && (
            <p>
              Receipt ID: <code>{p.external_id}</code>
            </p>
          )}
          {p.external_url && (
            <ExternalLink url={p.external_url}>
              Open published post
            </ExternalLink>
          )}
          {!p.external_id && !p.external_url && (
            <p className="muted">No provider receipt recorded.</p>
          )}
          {(p.provider === "manual" ||
            isOwnerAttestation(
              attempts.find(
                (attempt) =>
                  attempt.state === "published" ||
                  attempt.state === "reconciled",
              ) ?? {},
            )) &&
            p.state === "published" && (
              <p className="muted">
                Completion was confirmed by the owner, not verified by a
                provider API.
              </p>
            )}
        </section>
        {p.state === "queued" && (
          <QueueReplacement
            publication={p}
            post={post}
            media={media}
            action={action}
            pending={pending}
            pendingSchedule={pendingSchedule}
            pendingMedia={pendingMedia}
            reconcileMedia={reconcileMedia}
            onEdit={onEdit}
            onRefresh={onRefresh}
            onClose={onClose}
          />
        )}
        {p.state === "manual_required" && (
          <form
            className="form-stack"
            onSubmit={(e) => {
              e.preventDefault();
              void perform(
                "manual-complete",
                { external_url: url || undefined, note: note || undefined },
                "Handoff recorded",
              );
            }}
          >
            <div className="notice">
              <LinkIcon size={18} />
              <span>
                Publish this content in your network’s app, then record the
                result here. This button does not publish for you.
              </span>
            </div>
            {guidance && (
              <section className="detail-section">
                <h3>Current destination · {guidance.name}</h3>
                <p>{guidance.guidance}</p>
                {destination && (
                  <a
                    className="button button-secondary"
                    href={destination}
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    Open {guidance.name} <LinkIcon size={16} />
                  </a>
                )}
              </section>
            )}
            <Button
              type="button"
              variant="secondary"
              onClick={() =>
                void navigator.clipboard
                  .writeText(p.payload.text)
                  .then(() => setCopyStatus("Caption copied"))
                  .catch(() =>
                    setCopyStatus(
                      "Copy unavailable. Select and copy the caption above.",
                    ),
                  )
              }
            >
              <Copy size={16} />
              Copy post text
            </Button>
            {copyStatus && (
              <p role="status" className="muted">
                {copyStatus}
              </p>
            )}
            {p.payload.media_ids.map((id) => {
              const m = media.find(
                (m) => m.id === id && m.brand_id === p.brand_id,
              );
              return m ? (
                <a
                  className="button button-secondary"
                  key={id}
                  href={m.url}
                  download={m.name}
                >
                  Download {m.name}
                </a>
              ) : (
                <p key={id} className="muted">
                  Media {id} is unavailable in this library.
                </p>
              );
            })}
            <Field
              label="Published post URL"
              hint="Optional, but useful when checking the outcome later."
            >
              <input
                type="url"
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                placeholder="https://…"
              />
            </Field>
            <Field label="Completion note">
              <textarea
                value={note}
                onChange={(e) => setNote(e.target.value)}
                rows={2}
              />
            </Field>
            <Submit pending={pending}>I published this post</Submit>
          </form>
        )}
        {p.state === "uncertain" &&
          !["x", "facebook", "instagram", "threads", "linkedin"].includes(
            p.provider,
          ) && (
            <form
              className="form-stack"
              onSubmit={(e) => {
                e.preventDefault();
                void perform(
                  "reconcile",
                  { resolution, external_url: url || undefined, note },
                  "Outcome reconciled",
                );
              }}
            >
              <ErrorNotice message="The network may have accepted this post. Check the account before deciding its outcome; blindly retrying could create a duplicate." />
              <Field label="What did you verify?">
                <select
                  required
                  value={resolution}
                  onChange={(e) => setResolution(e.target.value)}
                >
                  <option value="">Choose verified outcome</option>
                  <option value="published">
                    The post exists on the network
                  </option>
                  <option value="not_published">
                    I verified it was not published
                  </option>
                </select>
              </Field>
              <Field label="Evidence / reconciliation note">
                <textarea
                  required
                  value={note}
                  onChange={(e) => setNote(e.target.value)}
                  rows={3}
                />
              </Field>
              <Field label="Post URL (if published)">
                <input
                  type="url"
                  value={url}
                  onChange={(e) => setUrl(e.target.value)}
                />
              </Field>
              <Submit pending={pending}>Record verified outcome</Submit>
            </form>
          )}
        {["queued", "manual_required"].includes(p.state) && (
          <form
            className="form-stack detail-section"
            onSubmit={(e) => {
              e.preventDefault();
              if (date)
                void perform(
                  "reschedule",
                  { scheduled_at: new Date(date).toISOString() },
                  "Post rescheduled",
                );
            }}
          >
            <Field label={`Reschedule · ${zone}`}>
              <input
                type="datetime-local"
                value={date}
                onChange={(e) => setDate(e.target.value)}
                required
              />
            </Field>
            <Button variant="secondary" disabled={pending}>
              Reschedule
            </Button>
          </form>
        )}
        {p.provider === "youtube" &&
          ["uncertain", "failed"].includes(p.state) && (
            <ProviderRecovery
              publication={p}
              onRetryAllowed={setRetryAllowed}
              action={action}
              pending={pending}
            />
          )}
        {p.provider === "x" && ["uncertain", "failed"].includes(p.state) && (
          <XRecovery
            publication={p}
            onRetryAllowed={setRetryAllowed}
            action={action}
            pending={pending}
          />
        )}
        {["facebook", "instagram", "threads", "linkedin"].includes(
          p.provider,
        ) &&
          ["uncertain", "failed"].includes(p.state) && (
            <MetaRecovery
              publication={p}
              onRetryAllowed={setRetryAllowed}
              action={action}
              pending={pending}
            />
          )}
        {p.state === "failed" && (
          <Button
            disabled={pending || !retryAllowed}
            onClick={() => void perform("retry", {}, "Retry queued")}
          >
            <RefreshCw size={16} />
            Retry publication
          </Button>
        )}
        {["queued", "manual_required", "failed"].includes(p.state) && (
          <Button
            variant="destructive"
            disabled={pending}
            onClick={() => void perform("cancel", {}, "Publication canceled")}
          >
            Cancel publication
          </Button>
        )}
        <AttemptHistory attempts={attempts} error={attemptError} />
      </div>
    </Dialog>
  );
}
