import { CalendarClock } from "lucide-react";
import type { Brand, Channel, Media } from "../api";
import { Network } from "./brand";
export function PostPreview({
  brand,
  channel,
  text,
  media,
}: {
  brand: Brand;
  channel?: Channel;
  text: string;
  media: Media[];
}) {
  return (
    <aside className="composer-preview">
      <div className="eyebrow">POST PREVIEW</div>
      <div className="preview-card">
        <div className="preview-author">
          <Network provider={channel?.provider ?? "manual"} />
          <div>
            <strong>{channel?.name ?? brand.name}</strong>
            <small>Preview · {channel?.provider ?? "shared version"}</small>
          </div>
        </div>
        <p className={text ? "" : "muted"}>
          {text ||
            "Your words will appear here. A little spark can go a long way."}
        </p>
        {media.map((m) =>
          m.mime.startsWith("image/") ? (
            <img
              className="preview-media"
              src={m.url}
              alt={m.name}
              key={m.id}
            />
          ) : (
            <video
              className="preview-media"
              src={m.url}
              key={m.id}
              controls
              preload="metadata"
            />
          ),
        )}
        <div className="preview-footer">
          Preview layout. Networks may display content differently.
        </div>
      </div>
      <div className="quiet-note">
        <CalendarClock size={19} />
        <p>
          {channel?.provider === "manual"
            ? "Manual channels become a handoff task when due. You publish in the network app, then record the outcome here."
            : "Publishing capabilities and media requirements vary by channel. The server checks your selected formats before scheduling."}
        </p>
      </div>
    </aside>
  );
}
