import { ChevronLeft, ChevronRight, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import { z } from "zod";
import { api, PublicationSchema, ScheduleSchema, type Publication, type Schedule } from "../api";
import { Network } from "../components/brand";
import { State, Time } from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";

type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;
const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
const dayNames = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
export function Calendar({
  publications,
  onCompose,
  channels,
  action,
  onRefresh,
}: {
  publications: Publication[];
  onCompose: (date: string) => void;
  channels?: any[];
  action?: Action;
  onRefresh?: () => Promise<void> | void;
}) {
  const [month, setMonth] = useState(
    () => new Date(new Date().getFullYear(), new Date().getMonth(), 1),
  );
  const [view, setView] = useState<"month" | "week">("month");
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
  const visible = publications.filter((p) => p.state !== "canceled");

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
        </div>
        <div className="calendar-toolbar-actions">
          <Button size="sm" onClick={() => onCompose(dateKey(new Date()))}>
            <Plus size={16} />
            New post
          </Button>
          <span className="muted">{zone}</span>
        </div>
      </div>
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
      <p className="muted calendar-hint">
        Select a day to see its posts or schedule something new. Click a day to add posts. Dates use your browser’s timezone.
      </p>
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
