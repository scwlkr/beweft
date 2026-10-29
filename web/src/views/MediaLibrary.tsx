import type { MediaLimits } from "../lib/api-contract.generated";
import { Play, Trash2, Upload } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { z } from "zod";
import {
  api,
  MediaSchema,
  ok,
  type ApiSession,
  type Brand,
  type Media,
} from "../api";
import { Empty, ErrorNotice, Field, Time } from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";
import { useMediaDescription } from "./useMediaDescription";

const LimitsSchema: z.ZodType<MediaLimits> = z.object({
  max_upload_bytes: z.number().positive(),
  quota_bytes: z.number().nonnegative(),
  used_bytes: z.number().nonnegative(),
  reserved_bytes: z.number().nonnegative(),
  available_bytes: z.number().nonnegative(),
});
type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;
export function MediaLibrary({
  brand,
  media,
  action,
  pending,
  reconcileMedia,
}: {
  brand: Brand;
  media: Media[];
  action: Action;
  pending: boolean;
  reconcileMedia: (media: Media, session: ApiSession) => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const selected = media.find((item) => item.id === selectedId);
  const [uploadError, setUploadError] = useState("");
  const [limit, setLimit] = useState<number | null>(null);
  useEffect(() => {
    api("/media/limits", LimitsSchema)
      .then((data) => setLimit(data.max_upload_bytes))
      .catch((e) => setUploadError(e.message));
  }, []);
  const limitLabel =
    limit === null
      ? "loading limits"
      : `${Math.floor(limit / 1024 / 1024)} MiB`;
  const upload = async (files: FileList | null) => {
    if (!files || limit === null) return;
    setUploadError("");
    for (const file of files) {
      if (file.size > limit) {
        setUploadError(
          `${file.name} is larger than the ${limitLabel} upload limit.`,
        );
        continue;
      }
      const body = new FormData();
      body.append("file", file);
      const done = await action(
        () => api(`/media?brand_id=${brand.id}`, MediaSchema, "POST", body),
        `${file.name} uploaded`,
      );
      if (!done) break;
    }
    if (input.current) input.current.value = "";
  };
  return (
    <>
      <div className="section-toolbar">
        <p className="muted">
          {media.length} files · Images and videos · Upload limit {limitLabel}
        </p>
        <Button
          disabled={pending || limit === null}
          onClick={() => input.current?.click()}
        >
          <Upload size={17} />
          {pending ? "Uploading…" : "Upload media"}
        </Button>
        <input
          ref={input}
          type="file"
          disabled={pending || limit === null}
          className="sr-only"
          tabIndex={-1}
          aria-label="Upload media files"
          accept="image/jpeg,image/png,image/webp,image/gif,video/mp4,video/webm"
          multiple
          onChange={(e) => void upload(e.target.files)}
        />
      </div>
      {uploadError && <ErrorNotice message={uploadError} />}
      <div
        onDragOver={(e) => e.preventDefault()}
        onDrop={(e) => {
          e.preventDefault();
          if (!pending) void upload(e.dataTransfer.files);
        }}
      >
        {media.length === 0 ? (
          <Empty
            title="Build your creative collection"
            action={
              <Button
                onClick={() => input.current?.click()}
                disabled={pending || limit === null}
              >
                <Upload size={17} />
                Choose files
              </Button>
            }
          >
            Drag photos and videos here, or choose files from your computer.
            Your media stays on your server.
          </Empty>
        ) : (
          <div className="media-grid">
            {media.map((m) => (
              <button
                className="media-card"
                key={m.id}
                onClick={() => {
                  setSelectedId(m.id);
                }}
              >
                <div className="media-image">
                  {m.mime.startsWith("image/") ? (
                    <img
                      src={m.url}
                      alt={m.alt_text || m.name}
                      loading="lazy"
                    />
                  ) : (
                    <>
                      <video src={m.url} preload="metadata" />
                      <span className="video-label">
                        <Play size={18} />
                        Video
                      </span>
                    </>
                  )}
                </div>
                <div className="media-description">
                  <strong>{m.name}</strong>
                  <small>
                    {(m.size / 1024 / 1024).toFixed(1)} MiB ·{" "}
                    {m.mime.split("/")[1].toUpperCase()}
                  </small>
                </div>
              </button>
            ))}
          </div>
        )}
      </div>
      {selected && (
        <MediaDetails
          key={selected.id}
          selected={selected}
          pending={pending}
          action={action}
          limit={limit}
          reconcileMedia={reconcileMedia}
          onClose={() => setSelectedId(null)}
        />
      )}
    </>
  );
}

function MediaDetails({
  selected,
  pending,
  action,
  limit,
  reconcileMedia,
  onClose,
}: {
  selected: Media;
  pending: boolean;
  action: Action;
  limit: number | null;
  reconcileMedia: (media: Media, session: ApiSession) => void;
  onClose: () => void;
}) {
  const description = useMediaDescription(selected, reconcileMedia);
  const record = description.media;
  const locked =
    pending || description.claiming || description.phase !== "ready";
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={record.name}
    >
      <div className="form-stack">
        {record.mime.startsWith("image/") ? (
          <img
            className="media-detail"
            src={record.url}
            alt={record.alt_text || record.name}
          />
        ) : (
          <video className="media-detail" controls src={record.url} />
        )}
        <p className="muted">
          {(record.size / 1024 / 1024).toFixed(2)} MiB · Uploaded{" "}
          <Time value={record.created_at} />
        </p>
        <Field label="Description">
          <textarea
            rows={3}
            value={description.text}
            disabled={
              pending ||
              description.claiming ||
              !["ready", "stale"].includes(description.phase)
            }
            readOnly={description.phase === "stale"}
            aria-label="Description"
            aria-describedby="media-description-help"
            onChange={(event) => description.setText(event.target.value)}
          />
        </Field>
        <small id="media-description-help" className="muted">
          Describe this media for people who cannot see it. New schedules save
          the current description; already scheduled posts keep their existing
          description.
        </small>
        <div className="inline-actions">
          <Button
            variant="secondary"
            disabled={
              locked ||
              description.text === record.alt_text ||
              Array.from(description.text).length > 2000
            }
            onClick={() => void description.send()}
          >
            Save description
          </Button>
          <small className="muted">
            {Array.from(description.text).length} / 2,000 characters
          </small>
        </div>
        {description.status && (
          <p role="status" className="muted">
            {description.status}
          </p>
        )}
        {description.error && <ErrorNotice message={description.error} />}
        {["pending", "blocked", "stale"].includes(description.phase) && (
          <div className="inline-actions">
            <Button
              variant="secondary"
              disabled={description.claiming}
              onClick={() =>
                void description.check(undefined, description.phase === "stale")
              }
            >
              {description.phase === "stale"
                ? "Use latest description"
                : "Check description"}
            </Button>
            {description.phase === "pending" && description.intent && (
              <Button
                variant="secondary"
                disabled={description.claiming}
                onClick={() => void description.send(description.intent!)}
              >
                Retry this description
              </Button>
            )}
          </div>
        )}
        {description.phase === "pending" && description.intent && (
          <p className="muted">
            Pending description: {description.intent.alt_text || "(empty)"}
          </p>
        )}
        {description.phase === "stale" && (
          <p className="muted">
            Last loaded description: {record.alt_text || "(empty)"}
          </p>
        )}
        {Array.from(description.text).length > 2000 && (
          <ErrorNotice message="Descriptions must be 2,000 characters or fewer." />
        )}
        <a
          className="button button-secondary"
          href={record.url}
          download={record.name}
        >
          Download original
        </a>
        <Button
          variant="destructive"
          disabled={locked || limit === null}
          onClick={() =>
            void action(
              () => api(`/media/${record.id}`, ok, "DELETE"),
              "Media deleted",
            ).then((ok) => {
              if (ok) onClose();
            })
          }
        >
          <Trash2 size={16} />
          Delete unused media
        </Button>
        <small className="muted">
          Media attached to a draft or publication is protected from deletion.
        </small>
      </div>
    </Dialog>
  );
}
