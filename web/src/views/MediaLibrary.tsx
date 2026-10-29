import type {
  MediaLimits,
  UpdateMediaRequest,
} from "../lib/api-contract.generated";
import { Play, Trash2, Upload } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { z } from "zod";
import { api, MediaSchema, ok, type Brand, type Media } from "../api";
import { Empty, ErrorNotice, Field, Time } from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";

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
}: {
  brand: Brand;
  media: Media[];
  action: Action;
  pending: boolean;
}) {
  const input = useRef<HTMLInputElement>(null);
  const selectedId = useRef<string | null>(null);
  const [selected, setSelected] = useState<Media | null>(null);
  const [description, setDescription] = useState("");
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
  const saveDescription = async () => {
    if (!selected || pending || Array.from(description).length > 2000) return;
    const id = selected.id;
    const input = { alt_text: description } satisfies UpdateMediaRequest;
    let updated: Media | undefined;
    const done = await action(async () => {
      updated = await api(`/media/${id}`, MediaSchema, "PATCH", input);
      return updated;
    }, "Description saved");
    if (done && updated) {
      setSelected((current) => (current?.id === id ? updated! : current));
      if (selectedId.current === id) setDescription(updated.alt_text);
    }
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
                  selectedId.current = m.id;
                  setSelected(m);
                  setDescription(m.alt_text);
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
        <Dialog
          open
          onOpenChange={(v) => {
            if (!v) {
              selectedId.current = null;
              setSelected(null);
            }
          }}
          title={selected.name}
        >
          <div className="form-stack">
            {selected.mime.startsWith("image/") ? (
              <img
                className="media-detail"
                src={selected.url}
                alt={selected.alt_text || selected.name}
              />
            ) : (
              <video className="media-detail" controls src={selected.url} />
            )}
            <p className="muted">
              {(selected.size / 1024 / 1024).toFixed(2)} MiB · Uploaded{" "}
              <Time value={selected.created_at} />
            </p>
            <Field label="Description">
              <textarea
                rows={3}
                value={description}
                disabled={pending}
                aria-label="Description"
                aria-describedby="media-description-help"
                onChange={(event) => setDescription(event.target.value)}
              />
            </Field>
            <small id="media-description-help" className="muted">
              Describe this media for people who cannot see it. New schedules
              save the current description; already scheduled posts keep their
              existing description.
            </small>
            <div className="inline-actions">
              <Button
                variant="secondary"
                disabled={
                  pending ||
                  description === selected.alt_text ||
                  Array.from(description).length > 2000
                }
                onClick={() => void saveDescription()}
              >
                Save description
              </Button>
              <small className="muted">
                {Array.from(description).length} / 2,000 characters
              </small>
            </div>
            {Array.from(description).length > 2000 && (
              <ErrorNotice message="Descriptions must be 2,000 characters or fewer." />
            )}
            <a
              className="button button-secondary"
              href={selected.url}
              download={selected.name}
            >
              Download original
            </a>
            <Button
              variant="destructive"
              disabled={pending || limit === null}
              onClick={() =>
                void action(
                  () => api(`/media/${selected.id}`, ok, "DELETE"),
                  "Media deleted",
                ).then((ok) => {
                  if (ok && selectedId.current === selected.id) {
                    selectedId.current = null;
                    setSelected(null);
                  }
                })
              }
            >
              <Trash2 size={16} />
              Delete unused media
            </Button>
            <small className="muted">
              Media attached to a draft or publication is protected from
              deletion.
            </small>
          </div>
        </Dialog>
      )}
    </>
  );
}
