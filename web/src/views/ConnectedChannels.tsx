import { AlertTriangle, CheckCircle2, Pause, Play, Search } from "lucide-react";
import { useState } from "react";
import {
  api,
  ChannelSchema,
  type Channel,
  type ManualGuidance,
  type Provider,
} from "../api";
import type { UpdateChannelRequest } from "../lib/api-contract.generated";
import { Network } from "../components/brand";
import { Button } from "../components/ui/button";
import { ChannelPermissions } from "./ChannelPermissions";
export function ConnectedChannels({
  channels,
  providers,
  pending,
  action,
  onReconnect,
  onDisconnect,
  platforms,
  platformsReady,
}: {
  channels: Channel[];
  providers: Provider[];
  pending: boolean;
  action: (fn: () => Promise<unknown>, message?: string) => Promise<boolean>;
  onReconnect: (provider: Provider, id: string) => void;
  onDisconnect: (channel: Channel) => void;
  platforms: ManualGuidance[];
  platformsReady: boolean;
}) {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<"all" | "healthy" | "attention">("all");
  const direct = channels.filter((c) => !["manual", "fixture"].includes(c.provider));
  const isAttention = (c: Channel) =>
    String(c.connection_status ?? "unknown") !== "connected" || c.connection_error != null;
  const healthyCount = direct.filter((c) => !isAttention(c)).length;
  const attentionCount = direct.filter(isAttention).length;
  const q = query.trim().toLowerCase();
  const visible = channels.filter((c) => {
    if (filter === "attention" && !isAttention(c)) return false;
    if (filter === "healthy" && isAttention(c)) return false;
    if (!q) return true;
    return c.name.toLowerCase().includes(q) || c.provider.toLowerCase().includes(q) || String(c.connection_status ?? "").toLowerCase().includes(q);
  });
  return (
    <>
      <div className="connection-health" role="status">
        <span className="chip ok"><CheckCircle2 size={14} /> {healthyCount} healthy</span>
        <span className={`chip ${attentionCount ? "bad" : ""}`}><AlertTriangle size={14} /> {attentionCount} need attention</span>
        <span className="chip">{channels.length} channels</span>
        {attentionCount > 0 && <button type="button" className="text-link" onClick={() => setFilter("attention")}>Show only attention</button>}
      </div>
      <div className="channel-filters">
        <label className="search-box">
          <Search size={16} />
          <input type="search" placeholder="Search channels…" value={query} onChange={(e) => setQuery(e.target.value)} aria-label="Search channels" />
        </label>
        <select value={filter} onChange={(e) => setFilter(e.target.value as typeof filter)} aria-label="Filter connection health">
          <option value="all">All health states</option>
          <option value="healthy">Healthy only</option>
          <option value="attention">Needs attention only</option>
        </select>
      </div>
      {visible.length === 0 ? (
        <p className="home-empty">No channels match this filter.</p>
      ) : (
      <div className="channel-list">
      {visible.map((c) => (
        <article className="channel-row" key={c.id}>
          <Network provider={c.provider} />
          <div>
            <h3>{c.name}</h3>
            {!["manual", "fixture"].includes(c.provider) && (
              <ChannelPermissions
                capabilities={c.capabilities}
                showScopes={c.provider === "google_business"}
              />
            )}
            {c.connection_error != null && (
              <p className="inline-error">{String(c.connection_error)}</p>
            )}
            <p>
              {c.provider === "manual"
                ? `Manual handoff · ${c.manual_guidance?.name ?? "Other platform"} · Owner confirms publication`
                : c.provider === "fixture"
                  ? "Isolated fixture · Does not publish"
                  : c.provider === "google_business"
                    ? "Google Business Profile"
                    : c.provider}{" "}
              · {c.timezone}
            </p>
            {c.provider === "manual" && (
              <div className="manual-destination">
                <label htmlFor={`manual-target-${c.id}`}>
                  Handoff destination
                </label>
                <select
                  id={`manual-target-${c.id}`}
                  aria-label={`Handoff destination for ${c.name}`}
                  value={c.target_platform ?? "generic"}
                  disabled={pending || !platformsReady}
                  onChange={(event) => {
                    const body = {
                      target_platform: event.target.value,
                    } satisfies UpdateChannelRequest;
                    void action(
                      () =>
                        api(`/channels/${c.id}`, ChannelSchema, "PATCH", body),
                      "Handoff destination updated",
                    );
                  }}
                >
                  {platformsReady ? (
                    <>
                      {!platforms.some(
                        (platform) =>
                          platform.id === (c.target_platform ?? "generic"),
                      ) && (
                        <option value={c.target_platform ?? "generic"}>
                          Current destination unavailable
                        </option>
                      )}
                      {platforms.map((platform) => (
                        <option key={platform.id} value={platform.id}>
                          {platform.name}
                        </option>
                      ))}
                    </>
                  ) : (
                    <option value={c.target_platform ?? "generic"}>
                      {c.manual_guidance?.name ?? "Other platform"}
                    </option>
                  )}
                </select>
                <small className="muted">
                  This guidance applies to existing handoffs for this channel
                  too.
                </small>
              </div>
            )}
          </div>
          {!["manual", "fixture"].includes(c.provider) && (
            <div className="inline-actions">
              <span className="status">
                Connection: {String(c.connection_status ?? "unknown")}
              </span>
              <Button
                variant="secondary"
                size="sm"
                disabled={
                  pending ||
                  !providers.some(
                    (p) =>
                      p.id === c.provider && p.implementation === "available",
                  )
                }
                onClick={() => {
                  const provider = providers.find((p) => p.id === c.provider);
                  if (provider) onReconnect(provider, c.id);
                }}
              >
                Reconnect
              </Button>
              <Button
                variant="ghost"
                size="sm"
                disabled={pending || c.connection_status === "disconnected"}
                onClick={() => {
                  onDisconnect(c);
                }}
              >
                Disconnect
              </Button>
            </div>
          )}
          <span className={`status ${c.paused ? "" : "status-published"}`}>
            {c.paused ? "Queue paused" : "Queue running"}
          </span>
          <Button
            variant="secondary"
            size="sm"
            disabled={pending}
            onClick={() =>
              void action(
                () =>
                  api(`/channels/${c.id}`, ChannelSchema, "PATCH", {
                    paused: !c.paused,
                  }),
                c.paused ? "Channel resumed" : "Channel paused",
              )
            }
          >
            {c.paused ? <Play size={15} /> : <Pause size={15} />}{" "}
            {c.paused ? "Resume" : "Pause"}
          </Button>
        </article>
      ))}
      </div>
      )}
    </>
  );
}
