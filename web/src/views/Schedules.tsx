import { Clock3, Plus, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { z } from "zod";
import { api, ok, ScheduleSchema, type Channel, type Schedule } from "../api";
import { ErrorNotice, Field, Submit } from "../components/shared";
import { Button } from "../components/ui/button";

type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;

const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;

function offsetMinutesAt(tz: string, date: Date): number {
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: tz,
    timeZoneName: "longOffset",
  }).formatToParts(date);
  const name = parts.find((p) => p.type === "timeZoneName")?.value ?? "";
  const m = name.match(/GMT([+-])(\d{2}):(\d{2})/);
  if (!m) return 0;
  const sign = m[1] === "-" ? -1 : 1;
  return sign * (parseInt(m[2], 10) * 60 + parseInt(m[3], 10));
}

function findNextDstTransition(
  tz: string,
): { date: Date; before: number; after: number } | null {
  const start = new Date();
  let prevOffset = offsetMinutesAt(tz, start);
  for (let i = 1; i <= 365; i++) {
    const d = new Date(start.getTime() + i * 24 * 60 * 60 * 1000);
    const noon = new Date(d);
    noon.setUTCHours(12, 0, 0, 0);
    const off = offsetMinutesAt(tz, noon);
    if (off !== prevOffset) {
      return { date: d, before: prevOffset, after: off };
    }
    prevOffset = off;
  }
  return null;
}

function formatOffset(mins: number): string {
  const sign = mins >= 0 ? "+" : "-";
  const abs = Math.abs(mins);
  const h = Math.floor(abs / 60);
  const m = abs % 60;
  return `UTC${sign}${h}${m ? `:${String(m).padStart(2, "0")}` : ""}`;
}

function utcTimeForWall(offsetMinutes: number, wallDate: Date, time: string): string {
  const [hh, mm] = time.split(":").map((n) => parseInt(n, 10));
  const wallMs = Date.UTC(
    wallDate.getUTCFullYear(),
    wallDate.getUTCMonth(),
    wallDate.getUTCDate(),
    hh || 0,
    mm || 0,
  );
  const utcMs = wallMs - offsetMinutes * 60 * 1000;
  return new Date(utcMs).toLocaleTimeString("en-US", {
    timeZone: "UTC",
    hour: "numeric",
    minute: "2-digit",
  });
}

type DstPreview =
  | { kind: "none" }
  | { kind: "invalid" }
  | {
      kind: "change";
      dateLabel: string;
      before: number;
      after: number;
      sampleTime: string;
      beforeUtc: string;
      afterUtc: string;
    };

function computeDstPreview(tz: string, sampleTime: string): DstPreview {
  try {
    const transition = findNextDstTransition(tz);
    if (!transition) return { kind: "none" };
    const beforeDay = new Date(transition.date.getTime() - 2 * 86400000);
    const afterDay = new Date(transition.date.getTime() + 2 * 86400000);
    const dateLabel = transition.date.toLocaleDateString(undefined, {
      timeZone: tz,
      month: "short",
      day: "numeric",
      year: "numeric",
    });
    return {
      kind: "change",
      dateLabel,
      before: transition.before,
      after: transition.after,
      sampleTime,
      beforeUtc: utcTimeForWall(transition.before, beforeDay, sampleTime),
      afterUtc: utcTimeForWall(transition.after, afterDay, sampleTime),
    };
  } catch {
    return { kind: "invalid" };
  }
}

export function Schedules({
  channels,
  action,
  pending,
}: {
  channels: Channel[];
  action: Action;
  pending: boolean;
}) {
  const [channel, setChannel] = useState(channels[0]?.id ?? "");
  const [rules, setRules] = useState<Schedule[]>([]);
  const [error, setError] = useState("");
  const [days, setDays] = useState([1, 2, 3, 4, 5]);
  const [time, setTime] = useState("09:00");
  const [timezone, setTimezone] = useState(channels[0]?.timezone ?? zone);
  const [goal, setGoal] = useState(5);
  const selected = channels.find((c) => c.id === channel) ?? channels[0];
  const selectedId = selected?.id ?? "";
  const currentChannel = useRef(selectedId);
  currentChannel.current = selectedId;
  const generation = useRef(0);
  useEffect(() => {
    setChannel(selectedId);
    setTimezone(selected?.timezone ?? zone);
    setRules([]);
    setError("");
    setGoal(5);
  }, [selectedId, selected?.timezone]);
  const load = useCallback(async () => {
    if (!selectedId || currentChannel.current !== selectedId) {
      setRules([]);
      return;
    }
    const request = ++generation.current;
    try {
      const next = await api(
        `/schedules?channel_id=${selectedId}`,
        z.array(ScheduleSchema),
      );
      if (
        currentChannel.current !== selectedId ||
        generation.current !== request
      )
        return;
      setRules(next);
      setError("");
    } catch (e) {
      if (
        currentChannel.current === selectedId &&
        generation.current === request
      )
        setError((e as Error).message);
    }
  }, [selectedId]);
  useEffect(() => {
    void load();
  }, [load]);

  const slotsCount = useMemo(
    () => rules.reduce((sum, r) => sum + (r.days?.length ?? 0), 0),
    [rules],
  );
  const sampleTime = (rules[0]?.time ?? "09:00").slice(0, 5);
  const preview = useMemo(
    () => computeDstPreview(timezone, sampleTime),
    [timezone, sampleTime],
  );

  const dayNames = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
  return (
    <section className="settings-section">
      <h2>A rhythm for each channel</h2>
      <p className="muted">
        Queue slots follow this channel’s timezone. Exact scheduled posts keep
        their selected time.
      </p>
      {channels.length === 0 ? (
        <p>Add a channel in Connections to set up its schedule.</p>
      ) : (
        <>
          <Field label="Channel">
            <select
              value={selectedId}
              onChange={(e) => {
                setChannel(e.target.value);
                setTimezone(
                  channels.find((c) => c.id === e.target.value)?.timezone ??
                    zone,
                );
              }}
            >
              {channels.map((c) => (
                <option value={c.id} key={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
          </Field>
          {error && <ErrorNotice message={error} />}
          <div className="posting-goal">
            <Field
              label="Weekly posting goal (posts/week)"
              hint="Client-side goal; not saved to the server."
            >
              <input
                type="number"
                min={1}
                max={50}
                value={goal}
                onChange={(e) => {
                  const n = parseInt(e.target.value, 10);
                  setGoal(Number.isFinite(n) && n > 0 ? n : 1);
                }}
              />
            </Field>
            <p className="muted">
              {slotsCount} of {goal} weekly slots set
              {slotsCount < goal
                ? ` — add ${goal - slotsCount} more to hit your goal.`
                : slotsCount > goal
                  ? " — above goal."
                  : " — goal met."}
            </p>
          </div>
          <div className="dst-preview">
            <strong>DST preview</strong>
            {preview.kind === "none" && (
              <p className="muted">
                No DST transition in the next 12 months for {timezone}.
              </p>
            )}
            {preview.kind === "invalid" && (
              <p className="muted">
                Enter a valid IANA timezone to preview DST.
              </p>
            )}
            {preview.kind === "change" && (
              <p className="muted">
                Next change {preview.dateLabel}: offset moves from{" "}
                {formatOffset(preview.before)} to {formatOffset(preview.after)}.
                A {preview.sampleTime} slot stays {preview.sampleTime} local but
                shifts from {preview.beforeUtc} to {preview.afterUtc} UTC.
              </p>
            )}
          </div>
          <div className="schedule-rules">
            {rules.length === 0 ? (
              <p className="muted">
                No recurring posting times yet. Add a time below to use the next
                available queue slot.
              </p>
            ) : (
              rules.map((r) => (
                <div className="schedule-rule" key={r.id}>
                  <Clock3 size={18} />
                  <div>
                    <strong>{r.time.slice(0, 5)}</strong>
                    <span>
                      {r.days.map((d) => dayNames[d - 1]).join(", ")} ·{" "}
                      {r.timezone}
                    </span>
                  </div>
                  <Button
                    variant="ghost"
                    size="icon"
                    disabled={pending}
                    aria-label={`Remove ${r.time} schedule`}
                    onClick={() =>
                      void action(
                        () => api(`/schedules/${r.id}`, ok, "DELETE"),
                        "Posting time removed",
                      ).then(async (done) => {
                        if (done) await load();
                      })
                    }
                  >
                    <Trash2 size={16} />
                  </Button>
                </div>
              ))
            )}
          </div>
          <form
            className="form-stack schedule-form"
            onSubmit={(e) => {
              e.preventDefault();
              if (!days.length) {
                setError("Choose at least one day.");
                return;
              }
              void action(
                () =>
                  api("/schedules", ScheduleSchema, "POST", {
                    channel_id: selectedId,
                    days,
                    time,
                    timezone,
                  }),
                "Posting time added",
              ).then(async (done) => {
                if (done) await load();
              });
            }}
          >
            <h3>Add a posting time</h3>
            <fieldset className="day-picker">
              <legend>Days of the week</legend>
              {dayNames.map((d, i) => (
                <button
                  type="button"
                  key={d}
                  aria-pressed={days.includes(i + 1)}
                  onClick={() =>
                    setDays(
                      days.includes(i + 1)
                        ? days.filter((n) => n !== i + 1)
                        : [...days, i + 1].sort(),
                    )
                  }
                >
                  {d}
                </button>
              ))}
            </fieldset>
            <div className="form-row">
              <Field label="Time">
                <input
                  type="time"
                  required
                  value={time}
                  onChange={(e) => setTime(e.target.value)}
                />
              </Field>
              <Field label="Timezone">
                <input
                  required
                  value={timezone}
                  onChange={(e) => setTimezone(e.target.value)}
                />
              </Field>
            </div>
            <Submit pending={pending}>
              <Plus size={16} />
              Add posting time
            </Submit>
          </form>
        </>
      )}
    </section>
  );
}
