import { useEffect, useState } from "react";
import { z } from "zod";
import { api } from "./api";
import {
  readScheduleIntent,
  scheduleEvent,
  type ScheduleIntent,
} from "./scheduleIntent";
export function usePendingSchedule(brandId: string) {
  const [key, setKey] = useState("");
  const [verifiedBrand, setVerifiedBrand] = useState("");
  const [intent, setIntent] = useState<ScheduleIntent | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    const controller = new AbortController();
    let storageKey = "";
    setKey("");
    setVerifiedBrand("");
    setIntent(null);
    setError("");
    const read = () => {
      if (!active || !storageKey) return;
      try {
        const pending = readScheduleIntent(storageKey);
        if (pending && pending.post.brand_id !== brandId)
          throw new Error("Pending schedule belongs to another brand.");
        setIntent(pending);
        setKey(storageKey);
        setVerifiedBrand(brandId);
        setError("");
      } catch (failure) {
        setKey("");
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
      controller.abort();
      window.removeEventListener("storage", read);
      window.removeEventListener(scheduleEvent, read);
    };
  }, [brandId]);
  const ready = !!key && verifiedBrand === brandId;
  return { key: ready ? key : "", intent: ready ? intent : null, error, ready };
}
