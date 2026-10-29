import { z } from "zod";
import {
  api,
  MediaSchema,
  requireWorkspaceSession,
  withWorkspaceSession,
  type ApiSession,
  type Media,
} from "./api";

export const MediaIntent = z.object({
  id: z.string().uuid(),
  brand_id: z.string().uuid(),
  media_id: z.string().uuid(),
  metadata_version: z.number().int().positive(),
  alt_text: z.string(),
});
export type MediaIntent = z.infer<typeof MediaIntent>;
export const mediaIntentEvent = "beweft-media-intent";
export const mediaIntentScope = (owner: string, brand: string) =>
  `beweft.media-description:${owner}:${brand}`;
export const mediaIntentPrefix = (owner: string, brand: string) =>
  `${mediaIntentScope(owner, brand)}:`;
// UUID spellings are case-insensitive. A noncanonical spelling of this scope
// is still ours, but cannot be treated as a canonical asset key.
export const isMediaIntentScopeKey = (key: string, scope: string) =>
  key.toLowerCase().startsWith(scope.toLowerCase());
export const mediaIntentKey = (owner: string, brand: string, media: string) =>
  `${mediaIntentPrefix(owner, brand)}${media}`;
export const isCanonicalMediaId = (id: string) =>
  id === id.toLowerCase() && z.string().uuid().safeParse(id).success;
export function parseMediaIntent(raw: string) {
  return MediaIntent.parse(JSON.parse(raw));
}
export function readMediaIntent(key: string) {
  const raw = localStorage.getItem(key);
  return raw === null ? null : parseMediaIntent(raw);
}

// A damaged record under a canonical asset key affects that asset only. If
// enumeration or a read fails, or the key cannot identify an asset, callers
// must fail closed globally because another pending operation may be hidden.
export function scanMediaIntents(owner: string, brand: string) {
  const scope = mediaIntentScope(owner, brand);
  const prefix = mediaIntentPrefix(owner, brand);
  const intents: Record<string, MediaIntent> = {};
  const assetErrors: Record<string, string> = {};
  const knownIds = new Set<string>();
  for (let i = 0; i < localStorage.length; i++) {
    const key = localStorage.key(i);
    if (key === null)
      throw new Error("Media description storage could not be enumerated.");
    if (!isMediaIntentScopeKey(key, scope)) continue;
    // The owner and brand are identifiable even if the asset delimiter or
    // identifier was lost. No asset can safely be exempted in that case.
    if (!key.startsWith(prefix))
      throw new Error("A pending media description could not be read safely.");
    const mediaId = key.slice(prefix.length);
    if (!isCanonicalMediaId(mediaId))
      throw new Error("A pending media description could not be read safely.");
    const raw = localStorage.getItem(key);
    if (raw === null) continue;
    knownIds.add(mediaId);
    try {
      const intent = parseMediaIntent(raw);
      if (intent.brand_id !== brand || intent.media_id !== mediaId)
        throw new Error("Media intent scope does not match its key.");
      intents[mediaId] = intent;
    } catch {
      assetErrors[mediaId] =
        "Pending description information could not be read. This asset stays blocked until it can be restored.";
    }
  }
  return { intents, assetErrors, knownIds };
}
export function announceMediaIntent(key: string) {
  window.dispatchEvent(new CustomEvent(mediaIntentEvent, { detail: key }));
}
// This lock protects a brief storage transition, not a media network request.
export async function lockMediaIntent<T>(
  key: string,
  session: ApiSession,
  signal: AbortSignal,
  work: () => T,
) {
  if (!navigator.locks)
    throw new Error("Description recovery requires HTTPS or localhost.");
  const wait = new AbortController();
  const cancel = () => wait.abort();
  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    wait.abort();
  }, 5000);
  if (signal.aborted) cancel();
  else signal.addEventListener("abort", cancel, { once: true });
  try {
    return await navigator.locks.request(key, { signal: wait.signal }, () =>
      withWorkspaceSession(
        session,
        () => {
          wait.signal.throwIfAborted();
          return work();
        },
        wait.signal,
      ),
    );
  } catch (failure) {
    if (timedOut)
      throw new Error("Media is busy. Check the description again.");
    throw failure;
  } finally {
    clearTimeout(timer);
    signal.removeEventListener("abort", cancel);
  }
}

// Lock in a stable order. The final schedule request runs inside these locks so
// another same-origin tab cannot claim a media edit between the check and the
// snapshot. Locks are not durable across reload; schedule recovery needs a
// separate server-side metadata precondition.
export async function withMediaScheduleGuard<T>(
  owner: string,
  brand: string,
  snapshots: Media[],
  session: ApiSession,
  signal: AbortSignal,
  reconcile: (media: Media, session: ApiSession) => void,
  work: () => Promise<T>,
): Promise<T> {
  if (!navigator.locks)
    throw new Error("Scheduling media safely requires HTTPS or localhost.");
  const ordered = [...snapshots].sort((a, b) =>
    a.id < b.id ? -1 : a.id > b.id ? 1 : 0,
  );
  const wait = new AbortController();
  const cancel = () => wait.abort();
  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    wait.abort();
  }, 5000);
  if (signal.aborted) cancel();
  else signal.addEventListener("abort", cancel, { once: true });
  const run = async (index: number): Promise<T> => {
    if (index < ordered.length)
      return navigator.locks.request(
        mediaIntentKey(owner, brand, ordered[index].id),
        { signal: wait.signal },
        () => run(index + 1),
      );
    clearTimeout(timer);
    await withWorkspaceSession(
      session,
      () => {
        signal.throwIfAborted();
        // Recheck the entire scope after waiting: a delayed storage event must
        // not hide an unidentifiable key or an unreadable record.
        const { intents, assetErrors } = scanMediaIntents(owner, brand);
        for (const item of ordered) {
          if (assetErrors[item.id])
            throw new Error(
              `The pending description for ${item.name} could not be read. It remains blocked.`,
            );
          if (intents[item.id])
            throw new Error(
              `A description for ${item.name} is still unconfirmed. Review it before scheduling.`,
            );
        }
      },
      signal,
    );
    const latest = await Promise.all(
      ordered.map((item) =>
        api(`/media/${item.id}`, MediaSchema, "GET", undefined, signal),
      ),
    );
    requireWorkspaceSession(session);
    signal.throwIfAborted();
    let changed = false;
    latest.forEach((item, index) => {
      const expected = ordered[index];
      if (item.id !== expected.id || item.brand_id !== brand)
        throw new Error("The media check returned a different asset.");
      if (item.metadata_version < expected.metadata_version)
        throw new Error(
          "The media check returned an older description. Check again before scheduling.",
        );
      reconcile(item, session);
      if (
        item.metadata_version !== expected.metadata_version ||
        item.alt_text !== expected.alt_text
      )
        changed = true;
    });
    if (changed)
      throw new Error(
        "Media descriptions changed. Review the updated descriptions, then schedule again.",
      );
    return work();
  };
  try {
    return await run(0);
  } catch (failure) {
    if (timedOut)
      throw new Error(
        "Media is busy. Check the descriptions and try scheduling again.",
      );
    throw failure;
  } finally {
    clearTimeout(timer);
    signal.removeEventListener("abort", cancel);
  }
}
