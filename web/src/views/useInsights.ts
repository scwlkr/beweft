import { useCallback, useEffect, useRef, useState } from "react";
import { z } from "zod";
import { api, type Brand, type Channel } from "../api";
import { Analytics, Inbox, ItemSchema, type Result } from "./insight-contracts";
export function useInsights(
  brand: Brand,
  channels: Channel[],
  kind: "analytics" | "inbox",
) {
  const [result, setResult] = useState<Result | null>(null);
  const [error, setError] = useState("");
  const [syncChannel, setSyncChannel] = useState<string | null>(null);
  const [metered, setMetered] = useState(false);
  const [busy, setBusy] = useState(false);
  const [reply, setReply] = useState<z.infer<typeof ItemSchema> | null>(null);
  const [syncResult, setSyncResult] = useState("");
  const contextKey = `${brand.id}:${kind}`;
  const currentContext = useRef(contextKey);
  currentContext.current = contextKey;
  const generation = useRef(0);
  useEffect(() => {
    currentContext.current = contextKey;
    return () => {
      currentContext.current = "";
    };
  }, [contextKey]);
  const load = useCallback(async () => {
    if (currentContext.current !== contextKey) return;
    const request = ++generation.current;
    try {
      const schema = kind === "analytics" ? Analytics : Inbox;
      const next = await api(
        `/${kind}?brand_id=${brand.id}`,
        schema as z.ZodType<Result>,
      );
      if (
        currentContext.current !== contextKey ||
        generation.current !== request
      )
        return;
      setResult(next);
      setError("");
    } catch (e) {
      if (
        currentContext.current === contextKey &&
        generation.current === request
      )
        setError((e as Error).message);
    }
  }, [brand.id, kind, contextKey]);
  useEffect(() => {
    setResult(null);
    setError("");
    setSyncResult("");
    setSyncChannel(null);
    setReply(null);
    void load();
  }, [load]);
  const sync = async () => {
    if (!syncChannel) return;
    setBusy(true);
    setError("");
    try {
      const result = await api(
        `/channels/${syncChannel}/sync`,
        z.object({
          metrics_observed: z.number(),
          inbox_items: z.number(),
          coverage: z.string(),
          errors: z.array(z.string()),
          note: z.string(),
        }),
        "POST",
        metered ? { allow_metered: true } : {},
      );
      if (currentContext.current !== contextKey) return;
      setSyncResult(
        `${result.errors.length ? "Synchronization incomplete" : "Synchronization finished"}: ${result.metrics_observed} metrics observed, ${result.inbox_items} inbox items. Coverage: ${result.coverage.replaceAll("_", " ")}. ${result.note}${result.errors.length ? ` Errors: ${result.errors.join("; ")}` : ""}`,
      );
      setSyncChannel(null);
      await load();
    } catch (e) {
      if (currentContext.current === contextKey) setError((e as Error).message);
    } finally {
      if (currentContext.current === contextKey) setBusy(false);
    }
  };
  const connected = channels.filter(
    (c) =>
      !["manual", "fixture"].includes(c.provider) &&
      c.connection_status === "connected",
  );
  return {
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
  };
}
