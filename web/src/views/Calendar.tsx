import { ChevronLeft, ChevronRight, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import { z } from "zod";
import { api, ScheduleSchema, type Publication, type Schedule } from "../api";
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
}: {
  publications: Publication[];
  onCompose: (date: string) => void;
  channels?: any[];
}) {
  const [month, setMonth] = useState(
    () => new Date(new Date().getFullYear(), new Date().getMonth(), 1),
  );
  const [selected, setSelected] = useState<string | null>(null);
  const [schedules, setSchedules] = useState<Schedule[]>([]);

  useEffect(() => {
    const ch = channels?.[0]?.id;
    if (!ch) return;
    api(`/schedules?channel_id=${ch}`, z.array(ScheduleSchema))
      .then(setSchedules)
      .catch(() => setSchedules([]));
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
  const shift = (n: number) =>
    setMonth(new Date(month.getFullYear(), month.getMonth() + n, 1));
  const visible = publications.filter((p) => p.state !== "canceled");
  return (
    <>
      <div className="calendar-toolbar">
        <div className="inline-actions">
          <h2>
            {month.toLocaleDateString(undefined, {
              month: "long",
              year: "numeric",
            })}
          </h2>
          <Button
            variant="ghost"
            size="icon"
            aria-label="Previous month"
            onClick={() => shift(-1)}
          >
            <ChevronLeft size={18} />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            aria-label="Next month"
            onClick={() => shift(1)}
          >
            <ChevronRight size={18} />
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={() =>
              setMonth(
                new Date(new Date().getFullYear(), new Date().getMonth(), 1),
              )
            }
          >
            Today
          </Button>
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
        {days.map((d) => {
          const items = visible.filter(
            (p) => dateKey(new Date(p.scheduled_at)) === dateKey(d),
          );
          const dow = d.getDay(); // 0 Sun
          const jsDow = dow === 0 ? 7 : dow;
          const slots = schedules
            .filter((s) => s.days.includes(jsDow))
            .map((s) => s.time.slice(0, 5));
          return (
            <button
              key={dateKey(d)}
              className={`calendar-day ${d.getMonth() !== month.getMonth() ? "outside" : ""} ${dateKey(d) === dateKey(new Date()) ? "today" : ""}`}
              onClick={() => setSelected(dateKey(d))}
              aria-label={`${d.toLocaleDateString()}, ${items.length} posts`}
            >
              <span className="day-number">{d.getDate()}</span>
              {slots.length > 0 && d.getMonth() === month.getMonth() && (
                <div className="calendar-slots">
                  {slots.slice(0, 2).map((t) => (
                    <span key={t} className="calendar-slot">
                      {t}
                    </span>
                  ))}
                  {slots.length > 2 && <span className="calendar-slot">+{slots.length - 2}</span>}
                </div>
              )}
              {items.slice(0, 3).map((p) => (
                <div className={`calendar-post calendar-${p.state}`} key={p.id}>
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
              {items.length > 3 && <small>+{items.length - 3} more</small>}
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
            ) && <p className="muted">No posts scheduled for this day.</p>}
            <Button
              onClick={() => {
                setSelected(null);
                onCompose(`${selected}T09:00`);
              }}
            >
              <Plus size={16} />
              Create post
            </Button>
          </div>
        </Dialog>
      )}
    </>
  );
}
