import { ArrowDown, ArrowUp, CalendarDays, Image, Plus } from "lucide-react";
import { useState } from "react";
import { z } from "zod";
import {
  api,
  PublicationSchema,
  type Channel,
  type Media,
  type Publication,
  type Post,
  type ApiSession,
} from "../api";
import { Network } from "../components/brand";
import { Empty, ExternalLink, State, Time } from "../components/shared";
import { Button } from "../components/ui/button";

import type { PendingMedia } from "../usePendingMedia";
import type { usePendingSchedule } from "../usePendingSchedule";
import { PublicationDetails } from "./PublicationDetails";
type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;
export function Queue({
  publications,
  allPublications,
  channels,
  media,
  posts,
  pendingSchedule,
  pendingMedia,
  reconcileMedia,
  onEdit,
  onRefresh,
  action,
  pending,
  onCompose,
}: {
  publications: Publication[];
  allPublications: Publication[];
  channels: Channel[];
  media: Media[];
  posts: Post[];
  pendingSchedule: ReturnType<typeof usePendingSchedule>;
  pendingMedia: PendingMedia;
  reconcileMedia: (media: Media, session: ApiSession) => void;
  onEdit: (post: Post) => void;
  onRefresh: () => Promise<void>;
  action: Action;
  pending: boolean;
  onCompose: () => void;
}) {
  const [view, setView] = useState("upcoming");
  const [channel, setChannel] = useState("all");
  const [selected, setSelected] = useState<Publication | null>(null);
  const history = ["published", "fixture_complete", "canceled"];
  const filtered = publications.filter(
    (p) =>
      (channel === "all" || p.channel_id === channel) &&
      (view === "history"
        ? history.includes(p.state)
        : view === "attention"
          ? ["failed", "uncertain", "manual_required"].includes(p.state)
          : !history.includes(p.state)),
  );
  const reorder = async (p: Publication, direction: number) => {
    const queue = allPublications
      .filter(
        (x) =>
          x.channel_id === p.channel_id &&
          x.state === "queued" &&
          x.scheduling_mode === "queue",
      )
      .sort((a, b) => a.scheduled_at.localeCompare(b.scheduled_at));
    const index = queue.findIndex((x) => x.id === p.id);
    if (index + direction < 0 || index + direction >= queue.length) return;
    [queue[index], queue[index + direction]] = [
      queue[index + direction],
      queue[index],
    ];
    await action(
      () =>
        api("/queue/reorder", z.array(PublicationSchema), "POST", {
          channel_id: p.channel_id,
          publication_ids: queue.map((x) => x.id),
        }),
      "Queue reordered",
    );
  };
  return (
    <>
      <div className="view-toolbar">
        <div className="tabs">
          <button
            aria-pressed={view === "upcoming"}
            onClick={() => setView("upcoming")}
          >
            Upcoming{" "}
            <span>
              {publications.filter((p) => !history.includes(p.state)).length}
            </span>
          </button>
          <button
            aria-pressed={view === "attention"}
            onClick={() => setView("attention")}
          >
            Needs attention
          </button>
          <button
            aria-pressed={view === "history"}
            onClick={() => setView("history")}
          >
            History
          </button>
        </div>
        <label className="sr-only" htmlFor="channel-filter">
          Filter channel
        </label>
        <select
          id="channel-filter"
          value={channel}
          onChange={(e) => setChannel(e.target.value)}
        >
          <option value="all">All channels</option>
          {channels.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
      </div>
      {filtered.length === 0 ? (
        <Empty
          title={
            view === "history"
              ? "Your story starts here"
              : view === "attention"
                ? "All clear for now"
                : "A little space for your next big idea"
          }
          action={
            view === "upcoming" ? (
              <Button onClick={onCompose}>
                <Plus size={17} />
                Create your first post
              </Button>
            ) : undefined
          }
        >
          {view === "history"
            ? "Publishing outcomes and receipts will appear here after your first post."
            : view === "attention"
              ? "Posts that need a handoff, reconnect, or reconciliation will appear here."
              : "Create a post, choose your channels, and find its place in the schedule."}
        </Empty>
      ) : (
        <div className="publication-list">
          {filtered.map((p) => (
            <article className="publication-card" key={p.id}>
              <div className="publication-time">
                <CalendarDays size={16} />
                <Time value={p.scheduled_at} />
              </div>
              <div className="publication-main">
                <div className="publication-author">
                  <Network provider={p.provider} />
                  <strong>{p.channel_name}</strong>
                  <State state={p.state} />
                  {channels.find((c) => c.id === p.channel_id)?.paused && (
                    <span className="status">Channel paused</span>
                  )}
                </div>
                {p.payload.title && <h3>{p.payload.title}</h3>}
                <p className="post-copy">{p.payload.text}</p>
                {p.payload.media_ids.length > 0 && (
                  <div className="post-attachments">
                    {p.payload.media_ids.map((id) => {
                      const m = media.find((m) => m.id === id);
                      return m?.mime.startsWith("image/") ? (
                        <img src={m.url} alt={m.name} key={id} />
                      ) : (
                        <span key={id}>
                          <Image size={15} />
                          {m?.name ?? "Attached media"}
                        </span>
                      );
                    })}
                  </div>
                )}
                {p.last_error && <p className="inline-error">{p.last_error}</p>}
                <div className="publication-actions">
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => setSelected(p)}
                  >
                    {p.state === "manual_required"
                      ? "Complete handoff"
                      : p.state === "uncertain"
                        ? "Reconcile outcome"
                        : "View details"}
                  </Button>
                  {p.external_url && (
                    <ExternalLink url={p.external_url}>View post</ExternalLink>
                  )}
                  {p.state === "queued" && p.scheduling_mode === "queue" && (
                    <div className="reorder-buttons">
                      <Button
                        variant="secondary"
                        size="sm"
                        disabled={pending}
                        onClick={() =>
                          void action(
                            () =>
                              api(
                                `/publications/${p.id}/prioritize`,
                                z.array(PublicationSchema),
                                "POST",
                                { scheduled_at: p.scheduled_at },
                              ),
                            "Moved to the first queue slot",
                          ).then(async (done) => {
                            if (!done) {
                              await onRefresh();
                              setSelected(p);
                            }
                          })
                        }
                      >
                        Move to front
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        aria-label={`Move ${p.payload.title || "post"} earlier`}
                        disabled={pending}
                        onClick={() => void reorder(p, -1)}
                      >
                        <ArrowUp size={15} />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        aria-label={`Move ${p.payload.title || "post"} later`}
                        disabled={pending}
                        onClick={() => void reorder(p, 1)}
                      >
                        <ArrowDown size={15} />
                      </Button>
                    </div>
                  )}
                </div>
              </div>
            </article>
          ))}
        </div>
      )}
      {selected && (
        <PublicationDetails
          media={media}
          post={posts.find((post) => post.id === selected.post_id)}
          pendingSchedule={pendingSchedule}
          pendingMedia={pendingMedia}
          reconcileMedia={reconcileMedia}
          onEdit={onEdit}
          onRefresh={onRefresh}
          channel={channels.find(
            (channel) => channel.id === selected.channel_id,
          )}
          publication={
            publications.find((p) => p.id === selected.id) ?? selected
          }
          onClose={() => setSelected(null)}
          action={action}
          pending={pending}
        />
      )}
    </>
  );
}
