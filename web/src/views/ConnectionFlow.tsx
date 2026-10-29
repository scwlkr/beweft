import { useEffect, useRef, useState } from "react";
import { z } from "zod";
import {
  api,
  captureWorkspaceSession,
  ChannelSchema,
  isCurrentWorkspaceSession,
  type ApiSession,
} from "../api";
import { ErrorNotice, Field } from "../components/shared";
import { Dialog } from "../components/ui/dialog";
import { Button } from "../components/ui/button";
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
  brandId,
  onDone,
}: {
  flowId: string;
  brandId: string;
  onDone: () => Promise<void>;
}) {
  const [flow, setFlow] = useState<z.infer<typeof FlowSchema> | null>(null);
  const [destination, setDestination] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [closed, setClosed] = useState(false);
  const alive = useRef(false);
  const running = useRef(false);
  const initialBrand = useRef(brandId);
  const currentBrand = useRef(brandId);
  currentBrand.current = brandId;
  const current = (session: ApiSession) =>
    alive.current &&
    currentBrand.current === initialBrand.current &&
    isCurrentWorkspaceSession(session);
  const close = () => {
    setClosed(true);
    const url = new URL(location.href);
    url.searchParams.delete("connection_flow");
    url.searchParams.delete("connection_error");
    history.replaceState(null, "", url.pathname + url.search + "#connections");
    void onDone();
  };
  useEffect(() => {
    alive.current = true;
    const controller = new AbortController();
    let session: ApiSession;
    try {
      session = captureWorkspaceSession();
    } catch {
      return () => {
        alive.current = false;
        controller.abort();
      };
    }
    api(
      `/connection-flows/${encodeURIComponent(flowId)}`,
      FlowSchema,
      "GET",
      undefined,
      controller.signal,
    )
      .then((found) => {
        if (!current(session) || controller.signal.aborted) return;
        if (found.id !== flowId || found.brand_id !== initialBrand.current) {
          setError(
            "This authorization belongs to a different brand or flow. Close this dialog and inspect provider access before starting again from the intended brand.",
          );
          return;
        }
        setFlow(found);
      })
      .catch((e) => {
        if (current(session) && !controller.signal.aborted) setError(e.message);
      });
    return () => {
      alive.current = false;
      controller.abort();
      if (!isCurrentWorkspaceSession(session)) {
        const url = new URL(location.href);
        if (url.searchParams.get("connection_flow") === flowId) {
          url.searchParams.delete("connection_flow");
          url.searchParams.delete("connection_error");
          history.replaceState(
            null,
            "",
            url.pathname + url.search + "#connections",
          );
        }
      }
    };
  }, [flowId]);
  useEffect(() => {
    if (brandId !== initialBrand.current) close();
  }, [brandId]);
  if (closed) return null;
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
          if (
            running.current ||
            !flow ||
            !destination ||
            flow.brand_id !== brandId
          )
            return;
          let session: ApiSession;
          try {
            session = captureWorkspaceSession();
          } catch {
            return;
          }
          if (!current(session)) return;
          running.current = true;
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
            if (current(session)) close();
          } catch (e) {
            if (current(session)) setError((e as Error).message);
          } finally {
            running.current = false;
            if (current(session)) setBusy(false);
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
        <Button type="submit" disabled={busy || !flow || !destination}>
          {busy ? "Connecting…" : "Finish connection"}
        </Button>
      </form>
    </Dialog>
  );
}
