import { z } from "zod";
export const Pending = z.object({
  request_id: z.string().uuid(),
  text: z.string(),
  expected_last_request_id: z.string().uuid().nullable().default(null),
});
export type Pending = z.infer<typeof Pending>;
export function readIntent(key: string) {
  const raw = localStorage.getItem(key);
  return raw ? Pending.parse(JSON.parse(raw)) : null;
}
export function forgetIntent(key: string, requestId: string) {
  if (readIntent(key)?.request_id === requestId) localStorage.removeItem(key);
}
export function lockIntent<T>(
  key: string,
  work: () => T | Promise<T>,
  signal?: AbortSignal,
): Promise<T> {
  if (!navigator.locks)
    throw new Error(
      "Reply safety requires a secure browser origin (HTTPS or localhost).",
    );
  return navigator.locks.request(key, { signal }, work);
}
