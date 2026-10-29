import { useEffect, useState } from "react";
import { z } from "zod";
import {
  api,
  captureWorkspaceSession,
  requireWorkspaceSession,
  type ApiSession,
} from "./api";
import {
  readScheduleIntent,
  scheduleEvent,
  type ScheduleIntent,
} from "./scheduleIntent";
export function usePendingSchedule(brandId: string, enabled = true) {
  const [key, setKey] = useState("");
  const [verifiedBrand, setVerifiedBrand] = useState("");
  const [verifiedSession, setVerifiedSession] = useState<ApiSession | null>(
    null,
  );
  const [intent, setIntent] = useState<ScheduleIntent | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    const controller = new AbortController();
    let storageKey = "";
    setKey("");
    setVerifiedBrand("");
    setVerifiedSession(null);
    setIntent(null);
    setError("");
    if (!enabled || !brandId) return;
    let session: ApiSession;
    try {
      session = captureWorkspaceSession();
    } catch (failure) {
      setError((failure as Error).message);
      return () => {
        active = false;
        controller.abort();
      };
    }
    const read = () => {
      if (!active || !storageKey) return;
      try {
        requireWorkspaceSession(session);
        const pending = readScheduleIntent(storageKey);
        if (pending && pending.post.brand_id !== brandId)
          throw new Error("Pending schedule belongs to another brand.");
        setIntent(pending);
        setKey(storageKey);
        setVerifiedBrand(brandId);
        setVerifiedSession(session);
        setError("");
      } catch (failure) {
        setKey("");
        setVerifiedBrand("");
        setVerifiedSession(null);
        setIntent(null);
        setError((failure as Error).message);
      }
    };
    void api(
      "/auth/me",
      z.object({ id: z.string() }),
      "GET",
      undefined,
      controller.signal,
    )
      .then((owner) => {
        if (!active) return;
        requireWorkspaceSession(session);
        storageKey = `beweft.pending-schedule:${owner.id}:${brandId}`;
        read();
      })
      .catch((failure) => {
        if (!active) return;
        try {
          requireWorkspaceSession(session);
        } catch {
          return;
        }
        setError(failure.message);
      });
    window.addEventListener("storage", read);
    window.addEventListener(scheduleEvent, read);
    return () => {
      active = false;
      controller.abort();
      window.removeEventListener("storage", read);
      window.removeEventListener(scheduleEvent, read);
    };
  }, [brandId, enabled]);
  const ready =
    enabled && !!key && verifiedBrand === brandId && verifiedSession !== null;
  return { key: ready ? key : "", intent: ready ? intent : null, error, ready };
}
