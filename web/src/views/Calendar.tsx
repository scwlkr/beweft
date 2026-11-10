import { ChevronLeft, ChevronRight, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import { z } from "zod";
import { api, PublicationSchema, ScheduleSchema, PostSchema, type Publication, type Schedule, type Post } from "../api";
import { Network } from "../components/brand";
import { State, Time } from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";

type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;
const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
function publicationTags(pub: Publication, posts: Post[]): string[] {
  const tags = new Set<string>();
  const add = (value: unknown) => {
    if (Array.isArray(value)) {
      for (const item of value) {
        if (typeof item === "string" && item.trim()) tags.add(item.trim());
      }
    }
  };
  add((pub as { tags?: unknown }).tags);
  add((pub.payload as { tags?: unknown })?.tags);
  add(pub.payload?.options?.tags);
  const post = posts.find((p) => p.id === pub.post_id);
  if (post) {
    add((post as { tags?: unknown }).tags);
    add(post.options?.tags);
  }
  return [...tags];
}
function postTags(post: Post): string[] {
  const tags = new Set<string>();
  const add = (value: unknown) => {
    if (Array.isArray(value)) {
      for (const item of value) {
        if (typeof item === "string" && item.trim()) tags.add(item.trim());
      }
    }
  };
  add((post as { tags?: unknown }).tags);
  add(post.options?.tags);
  return [...tags];
}
function tagMatches(tags: string[], filter: string) {
  if (!filter) return true;
  if (filter === "__untagged__") return tags.length === 0;
  return tags.includes(filter);
}
const dayNames = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
export function Calendar({
  publications,
  onCompose,
  channels,
  action,
  onRefresh,
  posts: postsProp,
}: {
  publications: Publication[];
  onCompose: (date: string) => void;
  channels?: any[];
  action?: Action;
  onRefresh?: () => Promise<void> | void;
  posts?: Post[];
}) {
  const [month, setMonth] = useState(
    () => new Date(new Date().getFullYear(), new Date().getMonth(), 1),
  );
  const [view, setView] = useState<"month" | "week">("month");
  const [layout, setLayout] = useState<"grid" | "agenda">("grid");
  const [tagFilter, setTagFilter] = useState("");
  const [weekStart, setWeekStart] = useState(() => {
    const now = new Date();
    const d = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    const day = (d.getDay() + 6) % 7;
    d.setDate(d.getDate() - day);
    d.setHours(0, 0, 0, 0);
    return d;
  });
  const [selected, setSelected] = useState<string | null>(null);
  const [schedules, setSchedules] = useState<Schedule[]>([]);
  const [posts, setPosts] = useState<Post[]>(postsProp ?? []);
  const [draggedId, setDraggedId] = useState<string | null>(null);

  useEffect(() => {
    const ids = (channels ?? []).map((c: any) => c.id).filter(Boolean);
    if (ids.length === 0) { setSchedules([]); return; }
    let cancelled = false;
    Promise.all(
      ids.map((id: string) =>
        api(`/schedules?channel_id=${id}`, z.array(ScheduleSchema)).catch(() => [] as Schedule[]),
      ),
    ).then((all) => { if (!cancelled) setSchedules(all.flat()); });
    return () => { cancelled = true; };
  }, [channels]);
  useEffect(() => {
    if (postsProp) { setPosts(postsProp); return; }
    const brandId = (channels ?? []).map((c: any) => c.brand_id).filter(Boolean)[0];
    if (!brandId) { setPosts([]); return; }
    let cancelled = false;
    api(`/posts?brand_id=${encodeURIComponent(brandId)}`, z.array(PostSchema))
      .then((data) => { if (!cancelled) setPosts(data); })
      .catch(() => { if (!cancelled) setPosts([]); });
    return () => { cancelled = true; };
  }, [channels, postsProp]);
  const start = new Date(month);
  start.setDate(1 - ((start.getDay() + 6) % 7));
  const days = Array.from({ length: 42 }, (_, i) => {
    const d = new Date(start);
    d.setDate(start.getDate() + i);
    return d;
  });
  const dateKey = (d: Date) =>
    `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  const shift = (n: number) => {
    if (view === "week") {
      setWeekStart((prev) => {
        const d = new Date(prev);
        d.setDate(d.getDate() + n * 7);
        return d;
      });
    } else {
      setMonth(new Date(month.getFullYear(), month.getMonth() + n, 1));
    }
  };
  const goToday = () => {
    if (view === "week") {
      const now = new Date();
      const d = new Date(now.getFullYear(), now.getMonth(), now.getDate());
      const day = (d.getDay() + 6) % 7;
      d.setDate(d.getDate() - day);
      d.setHours(0, 0, 0, 0);
      setWeekStart(d);
    } else {
      setMonth(new Date(new Date().getFullYear(), new Date().getMonth(), 1));
    }
  };
  const weekDays = Array.from({ length: 7 }, (_, i) => {
    const d = new Date(weekStart);
    d.setDate(weekStart.getDate() + i);
    return d;
  });
  const visible = publications.filter((p) => p.state !== "canceled" && tagMatches(publicationTags(p, posts), tagFilter));
  const knownTags = [
    ...new Set([
      ...publications.flatMap((p) => publicationTags(p, posts)),
      ...posts.flatMap((p) => postTags(p)),
    ]),
  ].sort((a, b) => a.localeCompare(b));
  const drafts = posts.filter(
    (p) => p.status === "draft" && tagMatches(postTags(p), tagFilter),
  );

  const reschedulePublication = async (pub: Publication, targetDate: string) => {
    if (!action) return;
    const currentTime = new Date(pub.scheduled_at);
    const hours = String(currentTime.getHours()).padStart(2, "0");
    const minutes = String(currentTime.getMinutes()).padStart(2, "0");
    const iso = new Date(`${targetDate}T${hours}:${minutes}:00`).toISOString();
    await action(
      () =>
        api(`/publications/${pub.id}/reschedule`, PublicationSchema, "POST", {
          scheduled_at: iso,
        }),
      "Post rescheduled",
    );
    if (onRefresh) await onRefresh();
  };

  return (
    <>
      <div className="calendar-toolbar">
        <div className="inline-actions">
          <h2>
            {view === "week"
              ? `${weekDays[0].toLocaleDateString(undefined, {
                  month: "short",
                  day: "numeric",
                })} – ${weekDays[6].toLocaleDateString(undefined, {
                  month: "short",
                  day: "numeric",
                  year: "numeric",
                })}`
              : month.toLocaleDateString(undefined, {
                  month: "long",
                  year: "numeric",
                })}
          </h2>
          <Button
            variant="ghost"
            size="icon"
            aria-label="Previous"
            onClick={() => shift(-1)}
          >
            <ChevronLeft size={18} />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            aria-label="Next"
            onClick={() => shift(1)}
          >
            <ChevronRight size={18} />
          </Button>
          <Button variant="secondary" size="sm" onClick={goToday}>
            Today
          </Button>
          <div className="view-toggle">
            <Button
              variant={view === "month" ? "secondary" : "ghost"}
              size="sm"
              onClick={() => setView("month")}
            >
              Month
            </Button>
            <Button
              variant={view === "week" ? "secondary" : "ghost"}
              size="sm"
              onClick={() => setView("week")}
            >
              Week
            </Button>
          </div>
          <div className="view-toggle">
            <Button
              variant={layout === "grid" ? "secondary" : "ghost"}
              size="sm"
              onClick={() => setLayout("grid")}
            >
              Grid
            </Button>
            <Button
              variant={layout === "agenda" ? "secondary" : "ghost"}
              size="sm"
              onClick={() => setLayout("agenda")}
            >
              Agenda
            </Button>
          </div>
        </div>
        <div className="calendar-toolbar-actions">
          <select value={tagFilter} onChange={(e) => setTagFilter(e.target.value)} aria-label="Filter by tag">
            <option value="">All tags</option>
            <option value="__untagged__">Untagged</option>
            {knownTags.map((tag) => (
              <option key={tag} value={tag}>
                {tag}
              </option>
            ))}
          </select>
          <Button size="sm" onClick={() => onCompose(dateKey(new Date()))}>
            <Plus size={16} />
            New post
          </Button>
          <span className="muted">{zone}</span>
        </div>
      </div>
      {layout === "agenda" ? (
        <section className="calendar-agenda" aria-label="Agenda">
          {(() => {
            const visibleDays = view === "month" ? days : weekDays;
            const dayKeys = new Set(visibleDays.map((d) => dateKey(d)));
            const items = visible
              .filter((p) => dayKeys.has(dateKey(new Date(p.scheduled_at))))
              .sort((a, b) => a.scheduled_at.localeCompare(b.scheduled_at));
            const grouped = new Map<string, Publication[]>();
            for (const p of items) {
              const key = dateKey(new Date(p.scheduled_at));
              if (!grouped.has(key)) grouped.set(key, []);
              grouped.get(key)!.push(p);
            }
            if (grouped.size === 0) {
              return <p className="muted">No dated posts in this range.</p>;
            }
            return [...grouped.entries()].map(([key, postsForDay]) => (
              <div className="agenda-day" key={key}>
                <h3>{new Date(`${key}T00:00:00`).toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric", year: "numeric" })}</h3>
                <ul className="agenda-list">
                  {postsForDay.map((p) => (
                    <li key={p.id} className={`agenda-item calendar-${p.state}`}>
                      <Time value={p.scheduled_at} />
                      <strong>{p.payload.title || p.payload.text || "Media post"}</strong>
                      <span className="muted">{p.channel_name}</span>
                      <State state={p.state} />
                    </li>
                  ))}
                </ul>
              </div>
            ));
          })()}
        </section>
      ) : (
      <div className="calendar-grid">
        {["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].map((d) => (
          <div className="calendar-weekday" key={d}>
            {d}
          </div>
        ))}
        {(view === "month" ? days : weekDays).map((d) => {
          const key = dateKey(d);
          const items = visible.filter(
            (p) => dateKey(new Date(p.scheduled_at)) === key,
          );
          const dow = d.getDay(); // 0 Sun
          const jsDow = dow === 0 ? 7 : dow;
          const slots = schedules
            .filter((s) => s.days.includes(jsDow))
            .map((s) => ({
              time: s.time.slice(0, 5),
              channel: (channels ?? []).find((c: any) => c.id === s.channel_id)?.name ?? "",
            }));
          const isOutsideMonth = view === "month" && d.getMonth() !== month.getMonth();
          const isToday = key === dateKey(new Date());
          return (
            <button
              key={key}
              className={`calendar-day ${isOutsideMonth ? "outside" : ""} ${isToday ? "today" : ""}`}
              onClick={() => setSelected(key)}
              onDragOver={(e) => {
                if (draggedId && action) {
                  e.preventDefault();
                }
              }}
              onDrop={(e) => {
                e.preventDefault();
                if (!draggedId || !action) return;
                const pub = publications.find((p) => p.id === draggedId);
                if (pub && ["queued", "manual_required"].includes(pub.state)) {
                  void reschedulePublication(pub, key);
                }
                setDraggedId(null);
              }}
              aria-label={`${d.toLocaleDateString()}, ${items.length} posts`}
            >
              <span className="day-number">{d.getDate()}</span>
              {slots.length > 0 && !isOutsideMonth && (
                <div className="calendar-slots">
                  {slots.slice(0, 2).map((s, i) => (
                    <span key={`${s.time}-${s.channel}-${i}`} className="calendar-slot" title={s.channel ? `${s.channel} slot ${s.time}` : `Slot ${s.time}`}>
                      {s.time}{s.channel ? ` · ${s.channel.slice(0, 8)}` : ""}
                    </span>
                  ))}
                  {slots.length > 2 && <span className="calendar-slot">+{slots.length - 2} slots</span>}
                </div>
              )}
              {items.slice(0, view === "week" ? 6 : 3).map((p) => (
                <div
                  className={`calendar-post calendar-${p.state}`}
                  key={p.id}
                  draggable={["queued", "manual_required"].includes(p.state) && !!action}
                  onDragStart={(e) => {
                    if (!action) return;
                    setDraggedId(p.id);
                    try {
                      e.dataTransfer.setData("text/plain", p.id);
                    } catch {}
                  }}
                  onDragEnd={() => setDraggedId(null)}
                  title="Drag to reschedule"
                >
                  <span>
                    {new Date(p.scheduled_at).toLocaleTimeString(undefined, {
                      hour: "numeric",
                      minute: "2-digit",
                    })}
                  </span>
                  <strong>
                    {p.payload.title || p.payload.text || "Media post"}
                  </strong>
                </div>
              ))}
              {items.length > (view === "week" ? 6 : 3) && (
                <small>+{items.length - (view === "week" ? 6 : 3)} more</small>
              )}
            </button>
          );
        })}
      </div>
      )}
      <p className="muted calendar-hint">
        Select a day to see its posts or schedule something new. Click a day to add posts. Dates use your browser’s timezone.
      </p>
      <section className="draft-tray" aria-label="Undated drafts">
        <h3>Undated drafts</h3>
        {drafts.length === 0 ? (
          <p className="muted">No undated drafts.</p>
        ) : (
          <ul className="draft-tray-list">
            {drafts.map((d) => (
              <li key={d.id} className="draft-tray-item">
                <strong>{d.title || d.text.slice(0, 60) || "Untitled draft"}</strong>
                <span className="muted">{d.text.slice(0, 80)}</span>
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => onCompose(dateKey(new Date()))}
                >
                  Schedule
                </Button>
              </li>
            ))}
          </ul>
        )}
      </section>
      {selected && (
        <Dialog
          open
          onOpenChange={(v) => !v && setSelected(null)}
          title="Posts for this day"
          description={new Date(`${selected}T00:00:00`).toLocaleDateString(undefined, {
            weekday: "long",
            month: "long",
            day: "numeric",
            year: "numeric",
          })}
        >
          <div className="form-stack">
            {visible
              .filter((p) => dateKey(new Date(p.scheduled_at)) === selected)
              .map((p) => (
                <div className="day-post" key={p.id}>
                  <Network provider={p.provider} />
                  <div>
                    <strong>{p.channel_name}</strong>
                    <p>{p.payload.title || p.payload.text || "Media post"}</p>
                    <Time value={p.scheduled_at} />
                  </div>
                  <State state={p.state} />
                </div>
              ))}
            {!visible.some(
              (p) => dateKey(new Date(p.scheduled_at)) === selected,
            ) && <p className="muted">Nothing scheduled for this day yet.</p>}
            <div className="inline-actions">
              <Button
                onClick={() => {
                  setSelected(null);
                  onCompose(`${selected}T09:00`);
                }}
              >
                <Plus size={16} />
                Create post
              </Button>
              <Button
                variant="secondary"
                onClick={() => {
                  setSelected(null);
                  onCompose(`${selected}T12:00`);
                }}
              >
                Schedule for afternoon
              </Button>
            </div>
          </div>
        </Dialog>
      )}
    </>
  );
}
