// A replace request has no idempotency key. Keep its old publication locked
// for review if the response is lost; never send a second request automatically.
const event = "beweft:replacement-intent";
export type ReplacementIntent = {
  publicationId: string;
  brandId: string;
  postId: string;
};
export const replacementKey = (scheduleKey: string) =>
  `${scheduleKey}:replacement`;
export function readReplacement(scheduleKey: string): ReplacementIntent | null {
  const raw = localStorage.getItem(replacementKey(scheduleKey));
  if (!raw) return null;
  const value: unknown = JSON.parse(raw);
  if (
    !value ||
    typeof value !== "object" ||
    !("publicationId" in value) ||
    typeof value.publicationId !== "string" ||
    !("brandId" in value) ||
    typeof value.brandId !== "string" ||
    !("postId" in value) ||
    typeof value.postId !== "string"
  )
    throw new Error(
      "A pending replacement cannot be read. Resolve it before scheduling.",
    );
  return value as ReplacementIntent;
}
export function saveReplacement(scheduleKey: string, value: ReplacementIntent) {
  const key = replacementKey(scheduleKey);
  const raw = JSON.stringify(value);
  localStorage.setItem(key, raw);
  if (localStorage.getItem(key) !== raw)
    throw new Error("Could not save the replacement review. Nothing was sent.");
  window.dispatchEvent(new Event(event));
}
export function clearReplacement(
  scheduleKey: string,
  value: ReplacementIntent,
) {
  const current = readReplacement(scheduleKey);
  if (
    !current ||
    current.publicationId !== value.publicationId ||
    current.brandId !== value.brandId ||
    current.postId !== value.postId
  )
    throw new Error("The pending replacement changed. Review it again.");
  localStorage.removeItem(replacementKey(scheduleKey));
  if (localStorage.getItem(replacementKey(scheduleKey)) !== null)
    throw new Error("Could not clear the pending replacement review.");
  window.dispatchEvent(new Event(event));
}
export const replacementEvent = event;
