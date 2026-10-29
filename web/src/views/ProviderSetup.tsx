import { ArrowUpRight, ShieldCheck } from "lucide-react";
import { useEffect, useState } from "react";
import { z } from "zod";
import { api, ChannelSchema, type Brand, type Provider } from "../api";
import { ErrorNotice, Field, Submit } from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";
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
  useEffect(() => {
    api("/provider-apps", z.array(AppSchema))
      .then((apps) => {
        const app = apps.find((a) => a.provider === id);
        if (app) {
          setConfigured(app.configured);
          setClientId(app.client_id);
          setInstance(app.instance_url ?? "");
        }
      })
      .catch((e) => setError(e.message));
  }, [id]);
  const save = async () => {
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
      setConfigured(true);
      setClientSecret("");
      setCallback(result.callback_url);
      setSaved("Provider app saved. You can now connect your account.");
      await onSaved();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const connect = async () => {
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
        setPassword("");
        await onSaved();
        onClose();
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
        const destination = new URL(flow.authorization_url);
        if (destination.protocol !== "https:")
          throw new Error("Provider returned an insecure authorization URL.");
        location.assign(destination.href);
      }
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
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
          <Submit pending={busy}>Save provider app</Submit>
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
            disabled={busy || (!configured && id !== "bluesky")}
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
