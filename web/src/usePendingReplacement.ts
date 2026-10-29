import { useCallback, useEffect, useRef, useState } from "react";
import { z } from "zod";
import {
  api,
  PublicationSchema,
  captureWorkspaceSession,
  isCurrentWorkspaceSession,
} from "./api";
import { scheduleLock } from "./scheduleIntent";
import {
  clearReplacement,
  readReplacement,
  replacementEvent,
  type ReplacementIntent,
} from "./replacementIntent";
export function usePendingReplacement(key: string, brandId: string) {
  const [intent, setIntent] = useState<ReplacementIntent | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const scope = useRef({ key, brandId });
  scope.current = { key, brandId };
  const read = useCallback(() => {
    if (!key) {
      setIntent(null);
      setError("");
      return;
    }
    try {
      const value = readReplacement(key);
      if (value && value.brandId !== brandId)
        throw new Error("Pending replacement belongs to another brand.");
      setIntent(value);
      setError("");
    } catch (failure) {
      setIntent(null);
      setError((failure as Error).message);
    }
  }, [key, brandId]);
  useEffect(() => {
    setBusy(false);
    read();
    window.addEventListener("storage", read);
    window.addEventListener(replacementEvent, read);
    return () => {
      window.removeEventListener("storage", read);
      window.removeEventListener(replacementEvent, read);
    };
  }, [read]);
  const check = async () => {
    if (!key || !intent || busy) return;
    let session;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    const active = () =>
      scope.current.key === key &&
      scope.current.brandId === brandId &&
      isCurrentWorkspaceSession(session);
    setBusy(true);
    try {
      await scheduleLock(
        key,
        async () => {
          const current = readReplacement(key);
          if (!current || current.publicationId !== intent.publicationId)
            throw new Error(
              "The pending replacement changed; review it again.",
            );
          const rows = await api(
            `/publications?brand_id=${encodeURIComponent(brandId)}`,
            z.array(PublicationSchema),
          );
          const original = rows.find((row) => row.id === current.publicationId);
          if (!active()) return;
          if (
            !original ||
            original.brand_id !== brandId ||
            original.post_id !== current.postId ||
            original.state === "queued"
          )
            throw new Error(
              "The original is still queued or unavailable. The replacement outcome is not confirmed; check the server before starting another publication.",
            );
          clearReplacement(key, current);
        },
        undefined,
        session,
      );
      if (active()) setError("");
    } catch (failure) {
      if (active()) setError((failure as Error).message);
    } finally {
      if (active()) setBusy(false);
    }
  };
  return { intent, error, busy, check };
}
