import { useComposer } from "./useComposer";
import { Check, ImagePlus, Save, Send, X } from "lucide-react";
import {
  type ApiSession,
  type Brand,
  type Channel,
  type Media,
  type Post,
} from "./api";
import { Network } from "./components/brand";
import { PostPreview } from "./components/PostPreview";
import { ErrorNotice, Field } from "./components/shared";
import { Button } from "./components/ui/button";
import { Dialog } from "./components/ui/dialog";
import { ProviderOptions } from "./ProviderOptions";
import type { PendingMedia } from "./usePendingMedia";
import type { usePendingSchedule } from "./usePendingSchedule";
export function Composer({
  brand,
  channels,
  media,
  initial,
  initialDate,
  onClose,
  onSaved,
  pendingSchedule,
  pendingMedia,
  reconcileMedia,
  onReviewMedia,
}: {
  brand: Brand;
  channels: Channel[];
  media: Media[];
  initial: Partial<Post> | null;
  initialDate?: string;
  onClose: () => void;
  onSaved: () => Promise<void>;
  pendingSchedule: ReturnType<typeof usePendingSchedule>;
  pendingMedia: PendingMedia;
  reconcileMedia: (media: Media, session: ApiSession) => void;
  onReviewMedia: (id: string) => void;
}) {
  const {
    post,
    title,
    setTitle,
    setText,
    selected,
    setSelected,
    variants,
    setVariants,
    variantMedia,
    setVariantMedia,
    options,
    setOptions,
    tab,
    setTab,
    mediaOpen,
    setMediaOpen,
    mode,
    setMode,
    date,
    setDate,
    preservedExact,
    correction,
    correctionBusy,
    correctionConflict,
    chooseOriginalTime,
    retryCorrection,
    busy,
    error,
    intent,
    intentChanged,
    recovery,
    recoveryMessage,
    recover,
    release,
    active,
    currentText,
    previewChannel,
    activeMedia,
    setActiveMedia,
    selectedMedia,
    affectedMedia,
    missingMedia,
    scheduleBlocked,
    submit,
  } = useComposer({
    brand,
    channels,
    media,
    initial,
    initialDate,
    onSaved,
    onClose,
    pendingSchedule,
    pendingMedia,
    reconcileMedia,
  });
  const previewDiffers =
    !!intent?.media_versions &&
    Object.entries(intent.media_versions).some(
      ([id, version]) =>
        media.find((item) => item.id === id)?.metadata_version !== version,
    );
  return (
    <Dialog
      open
      onOpenChange={(v) => {
        if (!v && (!busy || !!intent)) onClose();
      }}
      title={post?.id ? "Edit post" : "Create a post"}
      description="One idea. A version that feels right on every channel."
      wide
    >
      <div className="composer-grid">
        <fieldset
          className="composer-editor"
          disabled={
            busy ||
            !pendingSchedule.ready ||
            !!intent ||
            recovery === "complete"
          }
          aria-label="Post editor"
          aria-busy={busy}
        >
          <div className="channel-picker" aria-label="Choose channels">
            {channels.map((c) => (
              <button
                type="button"
                key={c.id}
                aria-pressed={selected.includes(c.id)}
                className={`channel-chip ${selected.includes(c.id) ? "selected" : ""}`}
                onClick={() =>
                  setSelected(
                    selected.includes(c.id)
                      ? selected.filter((id) => id !== c.id)
                      : [...selected, c.id],
                  )
                }
              >
                <Network provider={c.provider} />
                {c.name}
                {selected.includes(c.id) && <Check size={14} />}
              </button>
            ))}
            {channels.length === 0 && (
              <p className="muted">
                You can save a draft now. Add a channel in Connections to
                schedule it.
              </p>
            )}
          </div>
          <Field
            label="Working title"
            hint="Used as the video title when publishing to YouTube; otherwise internal."
          >
            <input
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="Give this idea a name"
              maxLength={240}
            />
          </Field>
          <div
            className="tabs scroll-tabs"
            role="tablist"
            aria-label="Post variants"
          >
            <button
              role="tab"
              aria-selected={tab === "all"}
              onClick={() => setTab("all")}
            >
              Shared version
            </button>
            {channels
              .filter((c) => selected.includes(c.id))
              .map((c) => (
                <button
                  role="tab"
                  key={c.id}
                  aria-selected={tab === c.id}
                  onClick={() => setTab(c.id)}
                >
                  {c.name}
                  {variants[c.id] !== undefined && (
                    <span className="small-dot" />
                  )}
                </button>
              ))}
          </div>
          <label className="sr-only" htmlFor="post-text">
            {active ? `${active.name} text` : "Post text"}
          </label>
          <textarea
            id="post-text"
            className="compose-text"
            value={currentText}
            onChange={(e) =>
              tab === "all"
                ? setText(e.target.value)
                : setVariants({ ...variants, [tab]: e.target.value })
            }
            placeholder="What would you like to share?"
          />
          <div className="editor-tools">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setMediaOpen(!mediaOpen)}
            >
              <ImagePlus size={17} /> Add media
            </Button>
            {tab !== "all" && variants[tab] !== undefined && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  const next = { ...variants };
                  delete next[tab];
                  setVariants(next);
                }}
              >
                Use shared text
              </Button>
            )}
            <span className="muted">
              {Array.from(currentText).length} characters
            </span>
          </div>
          {mediaOpen && (
            <div className="media-picker">
              <h3>Media library</h3>
              {media.length === 0 ? (
                <p className="muted">Upload media in the library first.</p>
              ) : (
                <div className="media-picker-grid">
                  {media.map((m) => (
                    <button
                      type="button"
                      key={m.id}
                      className={activeMedia.includes(m.id) ? "selected" : ""}
                      aria-pressed={activeMedia.includes(m.id)}
                      onClick={() =>
                        setActiveMedia(
                          activeMedia.includes(m.id)
                            ? activeMedia.filter((id) => id !== m.id)
                            : [...activeMedia, m.id],
                        )
                      }
                    >
                      {m.mime.startsWith("image/") ? (
                        <img src={m.url} alt={m.alt_text || m.name} />
                      ) : (
                        <span>Video</span>
                      )}
                      <small>{m.name}</small>
                    </button>
                  ))}
                </div>
              )}
            </div>
          )}
          {selectedMedia.length > 0 && (
            <div className="attachment-strip">
              {selectedMedia.map((m) => (
                <div key={m.id}>
                  {m.mime.startsWith("image/") ? (
                    <img src={m.url} alt={m.alt_text || m.name} />
                  ) : (
                    <span>{m.name}</span>
                  )}
                  <button
                    type="button"
                    className="attachment-remove"
                    aria-label={`Remove ${m.name}`}
                    onClick={() =>
                      setActiveMedia(activeMedia.filter((id) => id !== m.id))
                    }
                  >
                    <X size={13} />
                  </button>
                </div>
              ))}
            </div>
          )}
          {selectedMedia.length > 0 && (
            <div className="media-descriptions">
              <strong>Media descriptions</strong>
              <p className="muted">
                These are the last descriptions loaded. Scheduling uses saved
                descriptions, which another browser can change.
              </p>
              <ul>
                {selectedMedia.map((item) => (
                  <li key={item.id}>
                    <strong>{item.name}:</strong>{" "}
                    {item.alt_text || "No description yet"}
                  </li>
                ))}
              </ul>
            </div>
          )}
          {active && (
            <ProviderOptions
              provider={active.provider}
              capabilities={active.capabilities}
              value={options[active.id] ?? {}}
              onChange={(v) => setOptions({ ...options, [active.id]: v })}
            />
          )}
          {tab !== "all" && variantMedia[tab] !== undefined && (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                const next = { ...variantMedia };
                delete next[tab];
                setVariantMedia(next);
              }}
            >
              Use shared media
            </Button>
          )}
          <div className="schedule-choice">
            <Field label="When should this go out?">
              <select
                value={mode}
                onChange={(e) => setMode(e.target.value as typeof mode)}
                disabled={
                  correctionBusy ||
                  correction.kind === "loading" ||
                  correction.kind === "error"
                }
              >
                <option value="draft">Save as draft</option>
                <option value="queue">Next available queue slot</option>
                <option value="exact">Choose a date and time</option>
              </select>
            </Field>
            {mode === "exact" && (
              <Field
                label={`Date and time · ${Intl.DateTimeFormat().resolvedOptions().timeZone}`}
              >
                <input
                  type="datetime-local"
                  value={date}
                  onChange={(e) => setDate(e.target.value)}
                  disabled={
                    correctionBusy ||
                    correction.kind === "loading" ||
                    correction.kind === "error"
                  }
                  step="0.001"
                  required
                />
                {preservedExact && (
                  <small>Exact saved time: {preservedExact}</small>
                )}
              </Field>
            )}
            {mode === "queue" && (
              <p className="muted">
                Each channel uses its own posting schedule. Set posting times in
                Settings.
              </p>
            )}
          </div>
          {correction.kind === "corrupt" && (
            <ErrorNotice message="The saved scheduling time could not be understood. Choose the queue or enter a date and time to review it before scheduling." />
          )}
          {correction.kind === "error" && (
            <div role="status">
              <p>{correction.message}</p>
              <Button variant="secondary" onClick={retryCorrection}>
                Check saved time again
              </Button>
            </div>
          )}
          {mode !== "draft" && !pendingMedia.ready && (
            <div role="status" className="quiet-note">
              <p>
                {pendingMedia.error || "Checking media description recovery…"}
              </p>
              {pendingMedia.error && (
                <Button variant="secondary" onClick={pendingMedia.retry}>
                  Check media again
                </Button>
              )}
            </div>
          )}
          {mode !== "draft" && affectedMedia.length > 0 && (
            <div role="status" className="quiet-note">
              <p>
                A selected media description is unconfirmed. Review it before
                scheduling.
              </p>
              {affectedMedia.map((id) =>
                pendingMedia.assetErrors[id] ? (
                  <p key={id}>
                    {media.find((item) => item.id === id)?.name ?? "Media"}:{" "}
                    {pendingMedia.assetErrors[id]}
                  </p>
                ) : null,
              )}
              {affectedMedia.map((id) => (
                <Button
                  key={id}
                  variant="secondary"
                  onClick={() => onReviewMedia(id)}
                >
                  Review{" "}
                  {media.find((item) => item.id === id)?.name ??
                    "media description"}
                </Button>
              ))}
              {affectedMedia.some((id) => !!pendingMedia.assetErrors[id]) && (
                <Button variant="secondary" onClick={pendingMedia.retry}>
                  Check media again
                </Button>
              )}
            </div>
          )}
          {mode !== "draft" && missingMedia.length > 0 && (
            <ErrorNotice message="A selected media asset could not be loaded. Refresh the workspace before scheduling." />
          )}
        </fieldset>
        <PostPreview
          brand={brand}
          channel={previewChannel}
          text={currentText}
          media={selectedMedia}
        />
      </div>
      {pendingSchedule.error && (
        <ErrorNotice
          message={`Schedule recovery is unavailable: ${pendingSchedule.error}`}
        />
      )}
      {intent && correctionConflict && (
        <div className="notice" role="status">
          <div>
            <strong>Review the saved scheduling choice</strong>
            <p>
              Original request:{" "}
              {intent.mode === "queue"
                ? "next available queue slot"
                : intent.scheduled_at}
              .
            </p>
            <p>
              Other saved choice:{" "}
              {correctionConflict.corrupt
                ? "unreadable"
                : correctionConflict.value?.mode === "queue"
                  ? "next available queue slot"
                  : (correctionConflict.value?.scheduled_at ?? "none")}
              .
            </p>
            <Button
              variant="secondary"
              disabled={busy || intentChanged}
              onClick={() => void chooseOriginalTime()}
            >
              Use original scheduling choice
            </Button>
          </div>
        </div>
      )}
      {(intent || recovery === "complete") && (
        <div className="notice" role="status">
          <div>
            <strong>
              {recovery === "complete"
                ? "Schedule confirmed"
                : "Original schedule needs review"}
            </strong>
            <p>
              {recoveryMessage ||
                (intent?.request_contract === "request-id-v2"
                  ? "The outcome is not yet confirmed. This saved revision and its channel choices are locked until you check, retry, or stop the original schedule. Stopping first checks whether it already completed."
                  : intent?.request_contract === "request-id-v1"
                    ? "This older request has no saved media versions. Check its outcome and keep this record; it cannot be retried safely."
                    : "This older request has no recovery ID. Check its destinations and keep this record; retrying could create a duplicate.")}
            </p>
            {previewDiffers && (
              <p>
                Media in the preview has changed or is unavailable. The original
                schedule may use earlier descriptions.
              </p>
            )}
          </div>
        </div>
      )}
      {error && <ErrorNotice message={error} />}
      <footer className="dialog-footer">
        <Button variant="ghost" onClick={onClose} disabled={busy && !intent}>
          {intent || recovery === "complete" ? "Close" : "Cancel"}
        </Button>
        {intent ? (
          <>
            <Button
              variant="secondary"
              onClick={() => void recover(false)}
              disabled={busy || !pendingSchedule.ready || intentChanged}
            >
              Check scheduling outcome
            </Button>
            {recovery !== "release" &&
              intent.request_contract === "request-id-v2" &&
              !["mismatch", "legacy"].includes(recovery) && (
                <Button
                  onClick={() => void recover(true)}
                  disabled={busy || !pendingSchedule.ready}
                >
                  Retry original schedule
                </Button>
              )}
            {intent.request_contract === "request-id-v2" &&
              !["mismatch", "legacy"].includes(recovery) && (
                <Button
                  onClick={() => void release()}
                  disabled={busy || !pendingSchedule.ready}
                >
                  {recovery === "release"
                    ? "Refresh descriptions"
                    : "Stop original schedule"}
                </Button>
              )}
          </>
        ) : (
          recovery !== "complete" && (
            <Button
              onClick={submit}
              disabled={
                busy ||
                !pendingSchedule.ready ||
                scheduleBlocked ||
                correctionBusy ||
                (mode !== "draft" && correction.kind !== "ready")
              }
            >
              {mode === "draft" ? <Save size={17} /> : <Send size={17} />}{" "}
              {busy
                ? "Saving…"
                : mode === "draft"
                  ? "Save draft"
                  : mode === "queue"
                    ? "Add to queue"
                    : "Schedule post"}
            </Button>
          )
        )}
      </footer>
    </Dialog>
  );
}
