import { useCallback, useEffect, useRef, useState } from "react";
import { z } from "zod";
import { api, captureWorkspaceSession, isCurrentWorkspaceSession } from "./api";
import type {
  PublicationSummary,
  ReplacementStatus,
} from "./lib/api-contract.generated";
import { canonicalScheduleTime, scheduleLock } from "./scheduleIntent";
import {
  clearReplacement,
  isCurrentReplacement,
  readReplacement,
  replacementEvent,
  sameReplacement,
  type ReplacementIntent,
} from "./replacementIntent";

const Summary: z.ZodType<PublicationSummary> = z.object({
  id: z.string().uuid(),
  post_id: z.string().uuid(),
  brand_id: z.string().uuid(),
  channel_id: z.string().uuid(),
  channel_name: z.string(),
  provider: z.string(),
  post_version: z.number().int(),
  scheduled_at: z.string(),
  scheduling_mode: z.enum(["queue", "exact"]),
  state: z.enum([
    "queued",
    "sending",
    "manual_required",
    "published",
    "fixture_complete",
    "failed",
    "uncertain",
    "canceled",
  ]),
  created_at: z.string(),
  updated_at: z.string(),
});
const Status: z.ZodType<ReplacementStatus> = z.object({
  original: Summary,
  replacement: Summary.nullable(),
});
type Review = {
  intent: ReplacementIntent;
  status: ReplacementStatus;
  rejected: boolean;
};
function belongs(intent: ReplacementIntent, row: PublicationSummary) {
  return (
    row.id === intent.publicationId &&
    row.brand_id === intent.brandId &&
    row.post_id === intent.postId &&
    (!isCurrentReplacement(intent) || row.channel_id === intent.channelId)
  );
}
function linkedMatches(intent: ReplacementIntent, status: ReplacementStatus) {
  if (
    !isCurrentReplacement(intent) ||
    !status.replacement ||
    status.original.state !== "canceled"
  )
    return false;
  const original = status.original,
    replacement = status.replacement;
  const expectedTime = canonicalScheduleTime(intent.scheduledAt);
  return (
    expectedTime !== null &&
    belongs(intent, original) &&
    original.post_version === intent.publicationPostVersion &&
    canonicalScheduleTime(original.scheduled_at) === expectedTime &&
    original.scheduling_mode === intent.schedulingMode &&
    replacement.id !== original.id &&
    replacement.brand_id === intent.brandId &&
    replacement.post_id === intent.postId &&
    replacement.channel_id === intent.channelId &&
    replacement.post_version === intent.postVersion
  );
}
function reviewable(intent: ReplacementIntent, status: ReplacementStatus) {
  return (
    (isCurrentReplacement(intent) && !!intent.rejection) ||
    ["canceled", "published", "fixture_complete"].includes(
      status.original.state,
    )
  );
}
export function usePendingReplacement(key: string, brandId: string) {
  const [intent, setIntent] = useState<ReplacementIntent | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [review, setReview] = useState<Review | null>(null);
  const scope = useRef({ key, brandId });
  scope.current = { key, brandId };
  const read = useCallback(() => {
    setReview(null);
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
    setReview(null);
    setError("");
    try {
      await scheduleLock(
        key,
        async () => {
          const current = readReplacement(key);
          if (!current || !sameReplacement(current, intent))
            throw new Error(
              "The pending replacement changed; review it again.",
            );
          const status = await api(
            `/publications/${current.publicationId}/replacement-status`,
            Status,
          );
          if (!active()) return;
          if (!belongs(current, status.original))
            throw new Error(
              "The original publication does not match the saved request.",
            );
          if (linkedMatches(current, status)) {
            clearReplacement(key, current);
            return;
          }
          if (reviewable(current, status)) {
            setReview({
              intent: current,
              status,
              rejected: isCurrentReplacement(current) && !!current.rejection,
            });
            setError(
              status.replacement
                ? "The linked replacement differs from the saved request, or the saved request lacks its full identity. Review it before continuing."
                : "Review the original publication and the recorded outcome before continuing.",
            );
            return;
          }
          throw new Error(
            `The original is ${status.original.state.replaceAll("_", " ")} and no matching replacement is confirmed. The request may still be running; scheduling stays blocked.`,
          );
        },
        undefined,
        session,
      );
    } catch (failure) {
      if (active()) setError((failure as Error).message);
    } finally {
      if (active()) setBusy(false);
    }
  };
  const resolve = async () => {
    if (!key || !review || busy) return;
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
    setError("");
    try {
      await scheduleLock(
        key,
        async () => {
          const current = readReplacement(key);
          if (!current || !sameReplacement(current, review.intent))
            throw new Error("The pending replacement changed; check it again.");
          const status = await api(
            `/publications/${current.publicationId}/replacement-status`,
            Status,
          );
          if (!active()) return;
          if (
            !belongs(current, status.original) ||
            !reviewable(current, status) ||
            JSON.stringify(status) !== JSON.stringify(review.status)
          )
            throw new Error(
              "The publication status changed. Check it and review the new outcome.",
            );
          clearReplacement(key, current);
        },
        undefined,
        session,
      );
    } catch (failure) {
      if (active()) {
        setReview(null);
        setError((failure as Error).message);
      }
    } finally {
      if (active()) setBusy(false);
    }
  };
  return { intent, error, busy, review, check, resolve };
}
