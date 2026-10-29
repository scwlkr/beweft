import { ArrowUpRight, ShieldCheck } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { z } from "zod";
import {
  api,
  captureWorkspaceSession,
  ChannelSchema,
  isCurrentWorkspaceSession,
  type ApiSession,
  type Brand,
  type Provider,
} from "../api";
import type { AcknowledgeRevocationRequest } from "../lib/api-contract.generated";
import { ErrorNotice, Field } from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";
const RemovalStatus = z.object({
  provider: z.string(),
  revocation_id: z.string().uuid().nullable(),
});
const AppSchema = z.object({
  provider: z.string(),
  client_id: z.string(),
  instance_url: z.string().nullable(),
  configured: z.boolean(),
  updated_at: z.string().optional(),
});
export function ProviderSetup({
  provider,
  brand,
  reconnectId,
  onClose,
  onSaved,
}: {
  provider: Provider;
  brand: Brand;
  reconnectId?: string;
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const id = provider.id ?? provider.provider ?? "";
  const googleFamily = ["youtube", "google_business"].includes(id);
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");
  const [instance, setInstance] = useState(
    id === "bluesky" ? "https://bsky.social" : "",
  );
  const [configured, setConfigured] = useState(false);
  const [callback, setCallback] = useState(
    `${location.origin}/api/v1/connections/${id}/callback`,
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState("");
  const [identifier, setIdentifier] = useState("");
  const [password, setPassword] = useState("");
  // undefined means the current boundary has not been checked.
  const [removal, setRemoval] = useState<string | null | undefined>(undefined);
  const [removalBusy, setRemovalBusy] = useState(false);
  const [removalError, setRemovalError] = useState("");
  const [attested, setAttested] = useState(false);
  const alive = useRef(false);
  const removalGeneration = useRef(0);
  const current = (session: ApiSession) =>
    alive.current && isCurrentWorkspaceSession(session);
  const checkRemoval = async () => {
    let session: ApiSession;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    const generation = ++removalGeneration.current;
    const active = () =>
      current(session) && generation === removalGeneration.current;
    setRemoval(undefined);
    setAttested(false);
    setRemovalError("");
    setRemovalBusy(true);
    try {
      const status = await api(
        `/connections/${id}/removal-status`,
        RemovalStatus,
      );
      if (!active()) return;
      if (status.provider !== id)
        throw new Error(
          "Provider removal status did not match this connection.",
        );
      setRemoval(status.revocation_id);
    } catch (failure) {
      if (active()) setRemovalError((failure as Error).message);
    } finally {
      if (active()) setRemovalBusy(false);
    }
  };
  useEffect(() => {
    alive.current = true;
    let session: ApiSession;
    try {
      session = captureWorkspaceSession();
    } catch {
      return () => {
        alive.current = false;
      };
    }
    const controller = new AbortController();
    api(
      "/provider-apps",
      z.array(AppSchema),
      "GET",
      undefined,
      controller.signal,
    )
      .then((apps) => {
        if (!current(session) || controller.signal.aborted) return;
        const app = apps.find((a) => a.provider === id);
        if (app) {
          setConfigured(app.configured);
          setClientId(app.client_id);
          setInstance(app.instance_url ?? "");
        }
      })
      .catch((e) => {
        if (current(session) && !controller.signal.aborted) setError(e.message);
      });
    void checkRemoval();
    return () => {
      alive.current = false;
      removalGeneration.current++;
      controller.abort();
    };
  }, [id]);
  const acknowledgeRemoval = async () => {
    if (!removal || !attested || busy || removalBusy) return;
    let session: ApiSession;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    const observedId = removal;
    const generation = ++removalGeneration.current;
    const active = () =>
      current(session) && generation === removalGeneration.current;
    setRemoval(undefined);
    setAttested(false);
    setRemovalError("");
    setRemovalBusy(true);
    try {
      await api(
        `/connections/${id}/acknowledge-revocation`,
        z.object({ revocation_acknowledged: z.literal(true) }),
        "POST",
        {
          access_removed: true,
          revocation_id: observedId,
        } satisfies AcknowledgeRevocationRequest,
      );
      if (active()) {
        setRemoval(null);
        setSaved(
          "Your removal attestation was recorded. Provider access was not checked by beweft.",
        );
      }
    } catch (failure) {
      if (active())
        setRemovalError(
          `${(failure as Error).message} Check removal status and review it again before confirming.`,
        );
    } finally {
      if (active()) setRemovalBusy(false);
    }
  };
  const save = async () => {
    if (busy || removalBusy || removal !== null) return;
    let session: ApiSession;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    setBusy(true);
    setError("");
    setSaved("");
    try {
      const result = await api(
        `/provider-apps/${id}`,
        AppSchema.omit({ updated_at: true }).extend({
          callback_url: z.string(),
        }),
        "PUT",
        {
          client_id: clientId,
          ...(clientSecret ? { client_secret: clientSecret } : {}),
          ...(["mastodon", "bluesky"].includes(id)
            ? { instance_url: instance }
            : {}),
        },
      );
      if (!current(session)) return;
      setConfigured(true);
      setClientSecret("");
      setCallback(result.callback_url);
      setSaved("Provider app saved. You can now connect your account.");
      await onSaved();
    } catch (e) {
      if (current(session)) {
        setError((e as Error).message);
        setRemoval(undefined);
      }
    } finally {
      if (current(session)) setBusy(false);
    }
  };
  const connect = async () => {
    if (busy || removalBusy || removal !== null) return;
    let session: ApiSession;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    setBusy(true);
    setError("");
    try {
      if (id === "bluesky") {
        await api("/connections/bluesky", ChannelSchema, "POST", {
          brand_id: brand.id,
          identifier,
          app_password: password,
          reconnect_channel_id: reconnectId,
          timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
        });
        if (!current(session)) return;
        setPassword("");
        await onSaved();
        if (current(session)) onClose();
      } else {
        const flow = await api(
          `/connections/${id}/start`,
          z.object({
            authorization_url: z.string().url(),
            flow_id: z.string(),
          }),
          "POST",
          { brand_id: brand.id, reconnect_channel_id: reconnectId },
        );
        if (!current(session)) return;
        const destination = new URL(flow.authorization_url);
        if (destination.protocol !== "https:")
          throw new Error("Provider returned an insecure authorization URL.");
        location.assign(destination.href);
      }
    } catch (e) {
      if (current(session)) {
        setError((e as Error).message);
        setRemoval(undefined);
      }
    } finally {
      if (current(session)) setBusy(false);
    }
  };
  return (
    <Dialog
      open
      onOpenChange={(v) => !v && !busy && onClose()}
      title={`${reconnectId ? "Reconnect" : "Connect"} ${provider.name ?? id}`}
      description="Your credentials stay encrypted on your server."
    >
      <div className="form-stack">
        <p className="muted">
          {String(
            provider.reason ??
              "Provider app registration and account permission are required.",
          )}
        </p>
        {id === "google_business" && (
          <p className="muted">
            Requested Google scope:{" "}
            <code>https://www.googleapis.com/auth/business.manage</code>.
            Project approval and location eligibility remain unverified by
            connecting an account.
          </p>
        )}
        <dl className="connection-facts">
          <div>
            <dt>Adapter</dt>
            <dd>{String(provider.implementation ?? "unknown")}</dd>
          </div>
          <div>
            <dt>Verification</dt>
            <dd>{String(provider.verification ?? "unverified_live")}</dd>
          </div>
          <div>
            <dt>Supported formats</dt>
            <dd>
              {Array.isArray(provider.formats)
                ? provider.formats.map(String).join(", ")
                : "See provider setup"}
            </dd>
          </div>
        </dl>
        {error && <ErrorNotice message={error} />}
        <section
          className="form-stack detail-section"
          aria-label="Provider access removal"
        >
          <h3>Provider access removal</h3>
          {removal === undefined && (
            <p>
              Check the current removal status before configuring or connecting
              this provider.
            </p>
          )}
          {removal === null && (
            <p>
              No provider access removal is currently pending. This status does
              not verify access at the provider.
            </p>
          )}
          {removal && (
            <>
              {googleFamily ? (
                <>
                  <p>
                    Before removing access, inspect your other beweft YouTube
                    and Google Business Profile channels across brands. Google
                    authorization can affect current and prior Google app
                    projects and Google accounts. Remove only the relevant
                    beweft access in Google’s connected-app settings.
                  </p>
                  <p>
                    Other affected channels keep their stored credentials and
                    queues but are marked for reconnection. App changes and new
                    connections for both Google providers are blocked until the
                    pending removal is acknowledged.
                  </p>
                  <p>
                    Pending removal: <code>{removal}</code>
                  </p>
                </>
              ) : (
                <p>
                  Before connecting again, inspect your channels, including
                  other brands, and remove beweft access in the provider’s
                  connected-app settings or app passwords. Removing access can
                  affect other accounts.
                </p>
              )}
              <p>
                This is your attestation; beweft does not verify or revoke
                access at the provider.
              </p>
              <label className="checkbox-label">
                <input
                  type="checkbox"
                  checked={attested}
                  onChange={(event) => setAttested(event.target.checked)}
                />
                I removed beweft access at the provider and reviewed the impact
                on my other channels.
              </label>
              <Button
                type="button"
                variant="secondary"
                disabled={!attested || removalBusy || busy}
                onClick={() => void acknowledgeRemoval()}
              >
                Confirm provider access removed
              </Button>
            </>
          )}
          {removalError && <ErrorNotice message={removalError} />}
          <Button
            type="button"
            variant="ghost"
            disabled={removalBusy || busy}
            onClick={() => void checkRemoval()}
          >
            {removalBusy ? "Checking removal status…" : "Check removal status"}
          </Button>
        </section>
        <form
          className="form-stack detail-section"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <h3>1. Configure your provider app</h3>
          {id !== "bluesky" && (
            <>
              <Field label="Client / app ID">
                <input
                  value={clientId}
                  onChange={(e) => setClientId(e.target.value)}
                  required
                  autoComplete="off"
                />
              </Field>
              <Field
                label="Client / app secret"
                hint={
                  configured
                    ? "Leave blank to keep the existing secret. The saved value is never returned."
                    : "Stored only on your server, encrypted at rest."
                }
              >
                <input
                  value={clientSecret}
                  onChange={(e) => setClientSecret(e.target.value)}
                  type="password"
                  autoComplete="new-password"
                />
              </Field>
              <Field
                label="OAuth callback URL"
                hint="Register this exact URL with the provider. A public HTTPS origin is normally required."
              >
                <input
                  value={callback}
                  readOnly
                  onFocus={(e) => e.target.select()}
                />
              </Field>
            </>
          )}
          {["mastodon", "bluesky"].includes(id) && (
            <Field
              label="Instance URL"
              hint="The host must be public and included in your server’s BEWEFT_ALLOWED_PROVIDER_ORIGINS."
            >
              <input
                type="url"
                value={instance}
                required
                onChange={(e) => setInstance(e.target.value)}
              />
            </Field>
          )}
          <a
            className="text-link"
            href="https://github.com/scwlkr/beweft/blob/main/docs/research/provider-capabilities.md"
            target="_blank"
            rel="noreferrer"
          >
            Registration, permissions, and access requirements{" "}
            <ArrowUpRight size={14} />
          </a>
          <Button
            type="submit"
            disabled={busy || removalBusy || removal !== null}
          >
            {busy ? "Saving…" : "Save provider app"}
          </Button>
          {saved && (
            <p className="success-message" role="status">
              {saved}
            </p>
          )}
        </form>
        <form
          className="form-stack detail-section"
          onSubmit={(e) => {
            e.preventDefault();
            void connect();
          }}
        >
          <h3>2. Authorize your account</h3>
          {id === "bluesky" ? (
            <>
              <Field label="Bluesky identifier">
                <input
                  required
                  value={identifier}
                  onChange={(e) => setIdentifier(e.target.value)}
                  placeholder="you.bsky.social"
                />
              </Field>
              <Field
                label="App password"
                hint="Create a dedicated app password in Bluesky. The password is exchanged for a session and is not retained."
              >
                <input
                  required
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  autoComplete="new-password"
                />
              </Field>
            </>
          ) : (
            <p className="muted">
              Continue to {provider.name ?? id} to choose your account and grant
              access. Your current login stays active.
            </p>
          )}
          <Button
            type="submit"
            disabled={
              busy ||
              removalBusy ||
              removal !== null ||
              (!configured && id !== "bluesky")
            }
          >
            <ShieldCheck size={16} />
            {busy
              ? "Connecting…"
              : reconnectId
                ? "Reconnect account"
                : "Authorize account"}
          </Button>
        </form>
      </div>
    </Dialog>
  );
}
