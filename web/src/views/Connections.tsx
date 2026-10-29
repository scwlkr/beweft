import { ArrowUpRight, ChevronRight, Pause, Play, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import { z } from "zod";
import type { CreateChannelRequest } from "../lib/api-contract.generated";
import {
  api,
  captureWorkspaceSession,
  ChannelSchema,
  isCurrentWorkspaceSession,
  ManualGuidanceSchema,
  type Brand,
  type Channel,
  type Provider,
  type ManualGuidance,
} from "../api";
import { Network, Unicorn } from "../components/brand";
import { ErrorNotice, Field, Submit } from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";
import { ConnectedChannels } from "./ConnectedChannels";
import { ProviderSetup } from "./ProviderSetup";
import { DisconnectDialog } from "./DisconnectDialog";

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
  const [add, setAdd] = useState(false);
  const [name, setName] = useState("");
  const [timezone, setTimezone] = useState(zone);
  const [provider, setProvider] = useState("manual");
  const [target, setTarget] = useState("generic");
  const [platforms, setPlatforms] = useState<ManualGuidance[]>([]);
  const [platformError, setPlatformError] = useState("");
  const [platformAttempt, setPlatformAttempt] = useState(0);
  const [platformsReady, setPlatformsReady] = useState(false);
  const [detail, setDetail] = useState<Provider | null>(null);
  useEffect(() => {
    let active = true;
    const controller = new AbortController();
    let session;
    try {
      session = captureWorkspaceSession();
    } catch {
      return () => controller.abort();
    }
    setPlatformsReady(false);
    setPlatformError("");
    void api(
      "/manual-platforms",
      z.array(ManualGuidanceSchema),
      "GET",
      undefined,
      controller.signal,
    )
      .then((found) => {
        if (
          !active ||
          controller.signal.aborted ||
          !isCurrentWorkspaceSession(session)
        )
          return;
        if (!found.some((platform) => platform.id === "generic"))
          throw new Error(
            "The server did not include the generic manual destination.",
          );
        setPlatforms(found);
        setPlatformsReady(true);
      })
      .catch((failure) => {
        if (
          active &&
          !controller.signal.aborted &&
          isCurrentWorkspaceSession(session)
        ) {
          setTarget("generic");
          setPlatformError(failure.message);
        }
      });
    return () => {
      active = false;
      controller.abort();
    };
  }, [platformAttempt]);
  return (
    <>
      {setup && (
        <ProviderSetup
          key={`${setup.provider.id ?? setup.provider.provider}:${setup.reconnectId ?? "new"}`}
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
        <DisconnectDialog
          channel={disconnect}
          pending={pending}
          action={action}
          onClose={() => setDisconnect(null)}
          onReview={() => {
            const selected = disconnect;
            const selectedProvider = providers.find(
              (item) => item.id === selected.provider,
            ) ?? { id: selected.provider, name: selected.provider };
            setDisconnect(null);
            setSetup({ provider: selectedProvider, reconnectId: selected.id });
          }}
        />
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
            setDisconnect(channel);
          }}
          platforms={platforms}
          platformsReady={platformsReady}
        />
      )}
      {platformError && (
        <div className="inline-actions">
          <ErrorNotice
            message={`Manual destinations are unavailable: ${platformError}`}
          />
          <Button
            variant="secondary"
            onClick={() => setPlatformAttempt((attempt) => attempt + 1)}
          >
            Retry destinations
          </Button>
        </div>
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
                    ...(provider === "manual"
                      ? { target_platform: target }
                      : {}),
                  } satisfies CreateChannelRequest),
                "Channel added",
              ).then((ok) => {
                if (ok) {
                  setAdd(false);
                  setName("");
                  setTarget("generic");
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
            {provider === "manual" && (
              <Field label="Handoff destination">
                <select
                  aria-label="Handoff destination"
                  value={target}
                  disabled={!platformsReady}
                  onChange={(event) => setTarget(event.target.value)}
                >
                  {platformsReady ? (
                    platforms.map((platform) => (
                      <option key={platform.id} value={platform.id}>
                        {platform.name}
                      </option>
                    ))
                  ) : (
                    <option value="generic">Other platform</option>
                  )}
                </select>
              </Field>
            )}
            {provider === "manual" && !platformsReady && (
              <small className="muted">
                Other platform remains available while the destination list is
                unavailable.
              </small>
            )}
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
                  const destination = detail.id ?? detail.provider ?? "generic";
                  setProvider("manual");
                  setTarget(
                    platforms.some((platform) => platform.id === destination)
                      ? destination
                      : "generic",
                  );
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
