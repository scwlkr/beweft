import { Clock3, Plus, Trash2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { z } from "zod";
import { api, ok, ScheduleSchema, type Channel, type Schedule } from "../api";
import { ErrorNotice, Field, Submit } from "../components/shared";
import { Button } from "../components/ui/button";

type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;
const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
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
