import { useEffect, useState } from "react";
import { z } from "zod";
import { api, ChannelSchema } from "../api";
import { ErrorNotice, Field, Submit } from "../components/shared";
import { Dialog } from "../components/ui/dialog";
const FlowSchema = z.object({
  id: z.string(),
  brand_id: z.string(),
  provider: z.string(),
  destinations: z.array(
    z.object({
      id: z.string(),
      name: z.string(),
      username: z.string().nullable(),
    }),
  ),
});
export function ConnectionFlow({
  flowId,
  onDone,
}: {
  flowId: string;
  onDone: () => Promise<void>;
}) {
  const [flow, setFlow] = useState<z.infer<typeof FlowSchema> | null>(null);
  const [destination, setDestination] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const close = () => {
    const url = new URL(location.href);
    url.searchParams.delete("connection_flow");
    url.searchParams.delete("connection_error");
    history.replaceState(null, "", url.pathname + url.search + "#connections");
    void onDone();
  };
  useEffect(() => {
    api(`/connection-flows/${encodeURIComponent(flowId)}`, FlowSchema)
      .then(setFlow)
      .catch((e) => setError(e.message));
  }, [flowId]);
  return (
    <Dialog
      open
      onOpenChange={(v) => !v && !busy && close()}
      title="Choose your destination"
      description="Select the account or channel that should receive your posts."
    >
      <form
        className="form-stack"
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          setError("");
          try {
            await api(
              `/connection-flows/${encodeURIComponent(flowId)}/complete`,
              ChannelSchema,
              "POST",
              {
                destination_id: destination,
                timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
              },
            );
            close();
          } catch (e) {
            setError((e as Error).message);
          } finally {
            setBusy(false);
          }
        }}
      >
        {error && <ErrorNotice message={error} />}
        <Field label="Destination">
          <select
            required
            value={destination}
            onChange={(e) => setDestination(e.target.value)}
          >
            <option value="">Choose an account</option>
            {flow?.destinations.map((d) => (
              <option key={d.id} value={d.id}>
                {d.name}
                {d.username ? ` · ${d.username}` : ""}
              </option>
            ))}
          </select>
        </Field>
        {flow?.destinations.length === 0 && (
          <p className="muted">
            No eligible destinations were returned. Check your provider
            permissions and account type, then reconnect.
          </p>
        )}
        <p className="muted">
          Connecting verifies account identity. It does not verify publishing or
          grant permissions your provider has not approved.
        </p>
        <Submit pending={busy}>Finish connection</Submit>
      </form>
    </Dialog>
  );
}
