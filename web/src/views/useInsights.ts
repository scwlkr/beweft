import { useCallback, useEffect, useRef, useState } from "react";
import { z } from "zod";
import { api, type Brand, type Channel } from "../api";
import { Analytics, Inbox, ItemSchema, type Result } from "./insight-contracts";

export type AnalyticsRange = "7d" | "30d" | "90d" | "all";

const RANGE_DAYS: Record<Exclude<AnalyticsRange, "all">, number> = {
  "7d": 7,
  "30d": 30,
  "90d": 90,
};

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
  const [channelId, setChannelId] = useState("all");
  const [range, setRange] = useState<AnalyticsRange>("30d");
  const contextKey = `${brand.id}:${kind}:${channelId}:${range}`;
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
      const params = new URLSearchParams({ brand_id: brand.id });
      if (kind === "analytics") {
        if (channelId !== "all") params.set("channel_id", channelId);
        if (range !== "all") {
          const from = new Date(
            Date.now() - RANGE_DAYS[range] * 86_400_000,
          ).toISOString();
          params.set("from", from);
        }
        params.set("limit", "500");
      }
      const next = await api(
        `/${kind}?${params.toString()}`,
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
  }, [brand.id, channelId, kind, contextKey, range]);
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
    channelId,
    setChannelId,
    range,
    setRange,
  };
}
