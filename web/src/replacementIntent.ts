import { z } from "zod";
import { canonicalScheduleTime } from "./scheduleIntent";
import type { Publication } from "./api";

// A replace request has no idempotency key. Keep its original and exact input
// for review if the response is lost; never send another request automatically.
const event = "beweft:replacement-intent";
const base = z.object({
  publicationId: z.string().uuid(),
  brandId: z.string().uuid(),
  postId: z.string().uuid(),
});
const legacy = base.strict();
const current = base
  .extend({
    contract: z.literal("replace-v1"),
    channelId: z.string().uuid(),
    publicationPostVersion: z.number().int().positive().safe(),
    postVersion: z.number().int().positive().safe(),
    scheduledAt: z
      .string()
      .refine((value) => canonicalScheduleTime(value) !== null),
    schedulingMode: z.enum(["queue", "exact"]),
    rejection: z
      .object({ status: z.number().int(), code: z.string() })
      .strict()
      .optional(),
  })
  .strict();
export type ReplacementIntent =
  | z.infer<typeof legacy>
  | z.infer<typeof current>;
export type CurrentReplacementIntent = z.infer<typeof current>;
export const replacementKey = (scheduleKey: string) =>
  `${scheduleKey}:replacement`;
export function readReplacement(scheduleKey: string): ReplacementIntent | null {
  const raw = localStorage.getItem(replacementKey(scheduleKey));
  if (!raw) return null;
  try {
    return z.union([current, legacy]).parse(JSON.parse(raw));
  } catch {
    throw new Error(
      "A pending replacement cannot be read. Resolve it before scheduling.",
    );
  }
}
export function sameReplacement(a: ReplacementIntent, b: ReplacementIntent) {
  const normalize = (value: ReplacementIntent) =>
    isCurrentReplacement(value) ? current.parse(value) : legacy.parse(value);
  return JSON.stringify(normalize(a)) === JSON.stringify(normalize(b));
}
export function isCurrentReplacement(
  value: ReplacementIntent,
): value is CurrentReplacementIntent {
  return "contract" in value && value.contract === "replace-v1";
}
export function matchesReplacement(
  value: CurrentReplacementIntent,
  publication: Publication,
) {
  const expectedTime = canonicalScheduleTime(value.scheduledAt);
  return (
    expectedTime !== null &&
    publication.id !== value.publicationId &&
    publication.brand_id === value.brandId &&
    publication.post_id === value.postId &&
    publication.channel_id === value.channelId &&
    publication.post_version === value.postVersion &&
    publication.scheduling_mode === value.schedulingMode &&
    canonicalScheduleTime(publication.scheduled_at) === expectedTime
  );
}
export function saveReplacement(
  scheduleKey: string,
  value: CurrentReplacementIntent,
) {
  const key = replacementKey(scheduleKey);
  const raw = JSON.stringify(current.parse(value));
  localStorage.setItem(key, raw);
  if (localStorage.getItem(key) !== raw)
    throw new Error("Could not save the replacement review. Nothing was sent.");
  window.dispatchEvent(new Event(event));
}
export function recordReplacementRejection(
  scheduleKey: string,
  value: CurrentReplacementIntent,
  rejection: { status: number; code: string },
) {
  const stored = readReplacement(scheduleKey);
  if (!stored || !sameReplacement(stored, value))
    throw new Error("The pending replacement changed. Review it again.");
  const key = replacementKey(scheduleKey);
  const raw = JSON.stringify(current.parse({ ...value, rejection }));
  localStorage.setItem(key, raw);
  if (localStorage.getItem(key) !== raw)
    throw new Error("Could not save the rejection for review.");
  window.dispatchEvent(new Event(event));
}
export function clearReplacement(
  scheduleKey: string,
  value: ReplacementIntent,
) {
  const stored = readReplacement(scheduleKey);
  if (!stored || !sameReplacement(stored, value))
    throw new Error("The pending replacement changed. Review it again.");
  localStorage.removeItem(replacementKey(scheduleKey));
  if (localStorage.getItem(replacementKey(scheduleKey)) !== null)
    throw new Error("Could not clear the pending replacement review.");
  window.dispatchEvent(new Event(event));
}
export const replacementEvent = event;
