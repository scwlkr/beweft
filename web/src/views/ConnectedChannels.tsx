import { Pause, Play } from "lucide-react";
import { api, ChannelSchema, type Channel, type Provider } from "../api";
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
}: {
  channels: Channel[];
  providers: Provider[];
  pending: boolean;
  action: (fn: () => Promise<unknown>, message?: string) => Promise<boolean>;
  onReconnect: (provider: Provider, id: string) => void;
  onDisconnect: (channel: Channel) => void;
}) {
  return (
    <div className="channel-list">
      {channels.map((c) => (
        <article className="channel-row" key={c.id}>
          <Network provider={c.provider} />
          <div>
            <h3>{c.name}</h3>
            {!["manual", "fixture"].includes(c.provider) && (
              <ChannelPermissions capabilities={c.capabilities} />
            )}
            {c.connection_error != null && (
              <p className="inline-error">{String(c.connection_error)}</p>
            )}
            <p>
              {c.provider === "manual"
                ? "Manual handoff · Owner confirms publication"
                : c.provider === "fixture"
                  ? "Isolated fixture · Does not publish"
                  : c.provider}{" "}
              · {c.timezone}
            </p>
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
  );
}
