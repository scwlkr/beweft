import { useEffect, useState } from "react";
import { z } from "zod";
import { api } from "./api";
import {
  readScheduleIntent,
  scheduleEvent,
  type ScheduleIntent,
} from "./scheduleIntent";
export function usePendingSchedule(brandId: string, enabled: boolean) {
  const [key, setKey] = useState("");
  const [intent, setIntent] = useState<ScheduleIntent | null>(null);
  const [error, setError] = useState("");
  const [loadedBrand, setLoadedBrand] = useState("");
  useEffect(() => {
    let active = true;
    let storageKey = "";
    setKey("");
    setIntent(null);
    setError("");
    setLoadedBrand("");
    if (!enabled || !brandId) return;
    const read = () => {
      if (!active || !storageKey) return;
      try {
        const pending = readScheduleIntent(storageKey);
        if (pending && pending.post.brand_id !== brandId)
          throw new Error("Pending schedule belongs to another brand.");
        setIntent(pending);
        setKey(storageKey);
        setLoadedBrand(brandId);
        setError("");
      } catch (failure) {
        setKey("");
        setError((failure as Error).message);
      }
    };
    void api("/auth/me", z.object({ id: z.string() }))
      .then((owner) => {
        storageKey = `beweft.pending-schedule:${owner.id}:${brandId}`;
        read();
      })
      .catch((failure) => {
        if (active) setError(failure.message);
      });
    window.addEventListener("storage", read);
    window.addEventListener(scheduleEvent, read);
    return () => {
      active = false;
      window.removeEventListener("storage", read);
      window.removeEventListener(scheduleEvent, read);
    };
  }, [brandId, enabled]);
  const current = enabled && loadedBrand === brandId;
  return {
    key: current ? key : "",
    intent: current ? intent : null,
    error: current || !key ? error : "",
    ready: current && !!key,
  };
}
