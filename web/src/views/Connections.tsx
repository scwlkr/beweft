import { ArrowUpRight, ChevronRight, Pause, Play, Plus } from "lucide-react";
import { useState } from "react";
import { z } from "zod";
import {
  api,
  ChannelSchema,
  type Brand,
  type Channel,
  type Provider,
} from "../api";
import { Network, Unicorn } from "../components/brand";
import { Field, Submit } from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";
import { ConnectedChannels } from "./ConnectedChannels";
import { ProviderSetup } from "./ProviderSetup";

type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;
const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
export function Connections({
  brand,
  channels,
  providers,
  action,
  pending,
}: {
  brand: Brand;
  channels: Channel[];
  providers: Provider[];
  action: Action;
  pending: boolean;
}) {
  const [setup, setSetup] = useState<{
    provider: Provider;
    reconnectId?: string;
  } | null>(null);
  const [disconnect, setDisconnect] = useState<Channel | null>(null);
  const [disconnectResult, setDisconnectResult] = useState("");
  const [add, setAdd] = useState(false);
  const [name, setName] = useState("");
  const [timezone, setTimezone] = useState(zone);
  const [provider, setProvider] = useState("manual");
  const [detail, setDetail] = useState<Provider | null>(null);
  return (
    <>
      {setup && (
        <ProviderSetup
          brand={brand}
          provider={setup.provider}
          reconnectId={setup.reconnectId}
          onClose={() => setSetup(null)}
          onSaved={async () => {
            await action(async () => {}, "Connection settings updated");
          }}
        />
      )}
      {disconnect && (
        <Dialog
          open
          onOpenChange={(v) => !v && setDisconnect(null)}
          title={`Disconnect ${disconnect.name}?`}
          description="This pauses the channel, stops queued deliveries, and removes its stored credentials. Publishing history remains."
        >
          <div className="form-stack">
            <p className="muted">
              Some providers require revoking access in their own connected-app
              settings.
            </p>
            {disconnectResult && <p role="status">{disconnectResult}</p>}
            <Button
              variant="destructive"
              disabled={pending || !!disconnectResult}
              onClick={() =>
                void action(async () => {
                  const result = await api(
                    `/channels/${disconnect.id}/disconnect`,
                    z.object({
                      disconnected: z.boolean(),
                      provider_revoked: z.boolean(),
                      next_action: z.string(),
                    }),
                    "POST",
                    {},
                  );
                  setDisconnectResult(result.next_action);
                }, "Channel disconnected")
              }
            >
              Disconnect channel
            </Button>
          </div>
        </Dialog>
      )}
      <div className="section-toolbar">
        <h2>
          Connected channels{" "}
          <span className="count-pill">{channels.length}</span>
        </h2>
        <Button onClick={() => setAdd(true)}>
          <Plus size={17} />
          Add channel
        </Button>
      </div>
      {channels.length === 0 ? (
        <div className="connection-welcome">
          <div className="connection-illustration">
            <Network provider="youtube" />
            <Unicorn size={62} />
            <Network provider="x" />
          </div>
          <div>
            <h2>Your world, a little more connected</h2>
            <p>
              Connect a supported network, or add a manual handoff channel to
              plan and prepare posts for any platform.
            </p>
          </div>
        </div>
      ) : (
        <ConnectedChannels
          channels={channels}
          providers={providers}
          pending={pending}
          action={action}
          onReconnect={(provider, reconnectId) =>
            setSetup({ provider, reconnectId })
          }
          onDisconnect={(channel) => {
            setDisconnectResult("");
            setDisconnect(channel);
          }}
        />
      )}
      <section className="provider-section">
        <div className="section-heading">
          <h2>Bring your own connections</h2>
          <p>
            Provider app setup and account access are separate steps. See
            exactly what’s available before connecting.
          </p>
        </div>
        <div className="provider-grid">
          {providers
            .filter((p) => p.id !== "fixture")
            .map((p) => {
              const id = p.id ?? p.provider ?? "unknown";
              return (
                <button
                  className="provider-card"
                  key={id}
                  onClick={() =>
                    p.implementation === "available" && p.execution !== "manual"
                      ? setSetup({ provider: p })
                      : setDetail(p)
                  }
                >
                  <Network provider={id} />
                  <strong>{p.name ?? p.display_name ?? id}</strong>
                  <span className="provider-status">
                    {p.execution === "manual"
                      ? "Manual handoff"
                      : p.implementation === "available"
                        ? p.configuration === "ready"
                          ? "Ready to connect"
                          : "Setup required"
                        : "Not available"}
                  </span>
                  <ChevronRight size={16} />
                </button>
              );
            })}
        </div>
      </section>
      {add && (
        <Dialog
          open
          onOpenChange={(v) => !v && setAdd(false)}
          title="Add a channel"
          description="Manual channels let you prepare and schedule a handoff without connecting an account."
        >
          <form
            className="form-stack"
            onSubmit={(e) => {
              e.preventDefault();
              void action(
                () =>
                  api("/channels", ChannelSchema, "POST", {
                    brand_id: brand.id,
                    name,
                    provider,
                    timezone,
                  }),
                "Channel added",
              ).then((ok) => {
                if (ok) {
                  setAdd(false);
                  setName("");
                }
              });
            }}
          >
            <Field label="Channel type">
              <select
                value={provider}
                onChange={(e) => setProvider(e.target.value)}
              >
                <option value="manual">Manual handoff</option>
                {providers.some((p) => p.id === "fixture") && (
                  <option value="fixture">
                    Isolated fixture (development only)
                  </option>
                )}
              </select>
            </Field>
            <Field label="Channel name">
              <input
                required
                autoFocus
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g. My brand on Instagram"
                maxLength={100}
              />
            </Field>
            <Field
              label="Timezone"
              hint="Use an IANA timezone, such as America/Chicago or Europe/London."
            >
              <input
                required
                value={timezone}
                onChange={(e) => setTimezone(e.target.value)}
              />
            </Field>
            <div className="dialog-footer">
              <Submit pending={pending}>Add channel</Submit>
            </div>
          </form>
        </Dialog>
      )}
      {detail && (
        <Dialog
          open
          onOpenChange={(v) => !v && setDetail(null)}
          title={detail.name ?? detail.id ?? "Connection details"}
        >
          <div className="form-stack">
            <p>
              {String(
                detail.reason ??
                  "See server setup documentation for this network.",
              )}
            </p>
            <dl className="connection-facts">
              <div>
                <dt>Implementation</dt>
                <dd>{String(detail.implementation ?? "unknown")}</dd>
              </div>
              <div>
                <dt>Configuration</dt>
                <dd>{String(detail.configuration ?? "unknown")}</dd>
              </div>
              <div>
                <dt>Execution</dt>
                <dd>{String(detail.execution ?? "unknown")}</dd>
              </div>
              <div>
                <dt>Verification</dt>
                <dd>{String(detail.verification ?? "unverified")}</dd>
              </div>
            </dl>
            <p className="muted">
              An available adapter is not proof of live provider access. App
              registration, permissions, account eligibility, and provider
              review may still be required.
            </p>
            <a
              className="text-link"
              href="https://github.com/scwlkr/beweft/tree/main/docs"
              target="_blank"
              rel="noreferrer"
            >
              Connection setup documentation <ArrowUpRight size={15} />
            </a>
            {detail.execution === "manual" && (
              <Button
                onClick={() => {
                  setDetail(null);
                  setAdd(true);
                }}
              >
                Add manual channel
              </Button>
            )}
          </div>
        </Dialog>
      )}
    </>
  );
}
