import { CalendarDays, Check, CircleHelp, Download, RefreshCw, Send, Trophy } from "lucide-react";
import { useInsights, type AnalyticsRange } from "./useInsights";
import { z } from "zod";
import { api, type Brand, type Channel } from "../api";
import { Network } from "../components/brand";
import {
  Busy,
  Empty,
  ErrorNotice,
  ExternalLink,
  Field,
  Time,
} from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";
import { Reply } from "./InboxReply";

type Metric = {
  publication_id: string;
  channel_id: string;
  provider: string;
  text: string;
  external_url: string | null;
  observed_at: string;
  metrics: Array<{ name: string; value: number | null }>;
};

function scoreOf(m: Metric) {
  return m.metrics.reduce((sum, x) => sum + (x.value ?? 0), 0);
}
function csvEscape(v: string | number | null | undefined) {
  const s = String(v ?? "");
  return /[",\n]/.test(s) ? `"${s.replaceAll('"', '""')}"` : s;
}
export function Insights({
  brand,
  channels,
  kind,
}: {
  brand: Brand;
  channels: Channel[];
  kind: "analytics" | "inbox";
}) {
  const {
    result,
    error,
    setError,
    syncChannel,
    setSyncChannel,
    metered,
    setMetered,
    busy,
    reply,
    setReply,
    syncResult,
    load,
    sync,
    connected,
    channelId,
    setChannelId,
    range,
    setRange,
  } = useInsights(brand, channels, kind);
  const analytics = kind === "analytics" && "summary" in (result ?? {}) ? (result as unknown as { summary: { scheduled: number; published: number; attention: number }; metrics: Metric[] }) : null;
  const topPosts = (analytics?.metrics ?? []).slice().sort((a, b) => scoreOf(b) - scoreOf(a));
  const downloadCsv = () => {
    if (!analytics) return;
    const names = [...new Set(analytics.metrics.flatMap((m) => m.metrics.map((x) => x.name)))];
    const header = ["channel", "provider", "text", "external_url", "observed_at", ...names];
    const lines = analytics.metrics.map((m) => {
      const map = new Map(m.metrics.map((x) => [x.name, x.value]));
      return [channels.find((c) => c.id === m.channel_id)?.name ?? m.provider, m.provider, m.text, m.external_url ?? "", m.observed_at, ...names.map((n) => map.get(n) ?? "")].map(csvEscape).join(",");
    });
    const blob = new Blob([[header.map(csvEscape).join(","), ...lines].join("\n") + "\n"], { type: "text/csv;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url; a.download = `beweft-analytics-${range}.csv`; a.click();
    URL.revokeObjectURL(url);
  };
  return (
    <>
      {error && <ErrorNotice message={error} />}
      <div className="section-toolbar">
        <p className="muted">
          {kind === "analytics"
            ? "Provider metrics are snapshots with a recorded observation time."
            : "Replies go to the connected network. Read markers stay in beweft."}
        </p>
        <div className="inline-actions">
        {kind === "analytics" && analytics && analytics.metrics.length > 0 && (
          <Button variant="secondary" onClick={downloadCsv}>
            <Download size={16} /> Export CSV
          </Button>
        )}
        {connected.length > 0 && (
          <Button
            variant="secondary"
            onClick={() => {
              setMetered(false);
              setSyncChannel(connected[0].id);
            }}
          >
            <RefreshCw size={16} />
            Sync a channel
          </Button>
        )}
        </div>
      </div>
      {kind === "analytics" && (
        <div className="analytics-filters" role="group" aria-label="Analytics filters">
          <Field label="Channel">
            <select value={channelId} onChange={(e) => setChannelId(e.target.value)}>
              <option value="all">All channels</option>
              {channels.map((c) => (
                <option key={c.id} value={c.id}>{c.name}</option>
              ))}
            </select>
          </Field>
          <Field label="Date range (scheduled time)">
            <div className="view-toggle">
              {([["7d", "7 days"], ["30d", "30 days"], ["90d", "90 days"], ["all", "All time"]] as Array<[AnalyticsRange, string]>).map(([v, label]) => (
                <Button key={v} variant={range === v ? "secondary" : "ghost"} size="sm" onClick={() => setRange(v)} aria-pressed={range === v}>{label}</Button>
              ))}
            </div>
          </Field>
        </div>
      )}
      {syncResult && (
        <p className="availability-note" role="status">
          {syncResult}
        </p>
      )}
      {!result && !error ? (
        <Busy />
      ) : (
        result && (
          <>
            <div className="availability-note">
              <CircleHelp size={20} />
              <div>
                <strong>
                  {result.availability === "unavailable"
                    ? "Waiting for a supported connection"
                    : result.availability === "partial"
                      ? "Partial provider coverage"
                      : "Connected data"}
                </strong>
                <p>
                  {result.reason ??
                    "Availability depends on provider permissions and account eligibility."}
                </p>
              </div>
            </div>
            {analytics && (
              <>
                <div className="section-heading">
                  <h2>Publishing activity</h2>
                  <p>
                    Local workflow counts. Provider metrics and community data appear when supported connections sync.
                  </p>
                </div>
                <div className="activity-counts">
                  {[
                    { label: "Scheduled", value: analytics.summary.scheduled, icon: CalendarDays },
                    { label: "Published outcomes", value: analytics.summary.published, icon: Check },
                    { label: "Need attention", value: analytics.summary.attention, icon: CircleHelp },
                  ].map((s) => (
                    <div key={s.label}>
                      <s.icon size={20} />
                      <strong>{s.value}</strong>
                      <span>{s.label}</span>
                    </div>
                  ))}
                </div>
                {analytics.metrics.length > 0 && (
                  <>
                    <div className="section-heading">
                      <h2><Trophy size={18} style={{ display: "inline", verticalAlign: "-3px" }} /> Top posts</h2>
                      <p>Ranked by the sum of reported metric values (unavailable metrics count as 0 for ranking only). {analytics.metrics.length} posts in view.</p>
                    </div>
                    <div className="metrics-list top-posts">
                      {topPosts.slice(0, 5).map((m, i) => (
                        <article className="metric-card" key={m.publication_id}>
                          <div className="publication-author">
                            <span className="rank">#{i + 1}</span>
                            <Network provider={m.provider} />
                            <strong>{channels.find((c) => c.id === m.channel_id)?.name ?? m.provider}</strong>
                            <span className="status status-queued">{scoreOf(m).toLocaleString()} total</span>
                          </div>
                          <p className="post-copy">{m.text}</p>
                          <div className="inline-actions">
                            {m.external_url && <ExternalLink url={m.external_url}>View post</ExternalLink>}
                          </div>
                        </article>
                      ))}
                    </div>
                    <div className="section-heading"><h2>All metrics</h2><p>Sorted by most recent observation.</p></div>
                    <div className="metrics-list">
                      {analytics.metrics.map((m) => (
                        <article className="metric-card" key={m.publication_id}>
                          <div className="publication-author">
                            <Network provider={m.provider} />
                            <strong>{channels.find((c) => c.id === m.channel_id)?.name ?? m.provider}</strong>
                          </div>
                          <p className="post-copy">{m.text}</p>
                          <dl className="metric-values">
                            {m.metrics.map((metric) => (
                              <div key={metric.name}>
                                <dt>{metric.name.replaceAll("_", " ")}</dt>
                                <dd>{metric.value === null ? "Unavailable" : metric.value.toLocaleString()}</dd>
                              </div>
                            ))}
                          </dl>
                          <div className="inline-actions">
                            <small className="muted">Observed <Time value={m.observed_at} /></small>
                            {m.external_url && <ExternalLink url={m.external_url}>View post</ExternalLink>}
                          </div>
                        </article>
                      ))}
                    </div>
                  </>
                )}
                {analytics.metrics.length === 0 && (
                  <Empty title="No provider metrics yet">
                    When a supported provider supplies metrics, they appear here with their source and coverage. Unavailable metrics are never replaced with made-up numbers.
                  </Empty>
                )}
              </>
            )}
            {kind === "inbox" &&
              "items" in result &&
              (result.items.length === 0 ? (
                <Empty title="A quiet inbox, honestly shown">
                  No synchronized conversations are available. Sync a supported
                  channel, or check the network app when inbox access is
                  unavailable.
                </Empty>
              ) : (
                <div className="inbox-list">
                  {result.items.map((i) => (
                    <article
                      className={`inbox-item ${i.read_at ? "read" : ""}`}
                      key={i.id}
                    >
                      <Network provider={i.provider} />
                      <div className="inbox-message">
                        <div className="inbox-meta">
                          <strong>{i.item.author || "Account"}</strong>
                          <span>{i.channel_name}</span>
                          {!i.read_at && (
                            <span className="status status-queued">Unread</span>
                          )}
                        </div>
                        <p className="post-copy">{i.item.text}</p>
                        <div className="inline-actions">
                          <small className="muted">
                            Synced <Time value={i.synced_at} />
                          </small>
                          {i.item.url && (
                            <ExternalLink url={i.item.url}>
                              Open conversation
                            </ExternalLink>
                          )}
                          {!i.read_at && (
                            <Button
                              variant="ghost"
                              size="sm"
                              onClick={() =>
                                void api(
                                  `/inbox/${i.id}/read`,
                                  z.unknown(),
                                  "POST",
                                  {},
                                )
                                  .then(load)
                                  .catch((e) => setError(e.message))
                              }
                            >
                              Mark read
                            </Button>
                          )}
                          {i.can_reply && (
                            <Button
                              variant="secondary"
                              size="sm"
                              onClick={() => setReply(i)}
                            >
                              <Send size={14} />
                              Reply
                            </Button>
                          )}
                        </div>
                      </div>
                    </article>
                  ))}
                </div>
              ))}
          </>
        )
      )}
      {syncChannel && (
        <Dialog
          open
          onOpenChange={(v) => !v && !busy && setSyncChannel(null)}
          title="Sync channel data"
          description="Read recent supported metrics and conversations from the provider."
        >
          <div className="form-stack">
            <Field label="Channel">
              <select
                value={syncChannel}
                onChange={(e) => {
                  setSyncChannel(e.target.value);
                  setMetered(false);
                }}
              >
                {connected.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name}
                  </option>
                ))}
              </select>
            </Field>
            {channels.find((c) => c.id === syncChannel)?.provider === "x" && (
              <label className="checkbox-label">
                <input
                  type="checkbox"
                  checked={metered}
                  onChange={(e) => setMetered(e.target.checked)}
                />
                I authorize this metered X API read, which may incur provider
                charges.
              </label>
            )}
            <p className="muted">
              Coverage is bounded to recent records; a successful sync does not
              mean every historical conversation or metric is available.
            </p>
            {error && <ErrorNotice message={error} />}
            <Button
              disabled={
                busy ||
                (channels.find((c) => c.id === syncChannel)?.provider === "x" &&
                  !metered)
              }
              onClick={() => void sync()}
            >
              {busy ? "Synchronizing…" : "Sync now"}
            </Button>
          </div>
        </Dialog>
      )}
      {reply && (
        <Reply
          key={`${brand.id}:${reply.channel_id}:${reply.id}`}
          brandId={brand.id}
          item={reply}
          onClose={() => setReply(null)}
          onSent={load}
        />
      )}
    </>
  );
}
