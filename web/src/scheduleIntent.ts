import { z } from "zod";
import {
  api,
  ApiError,
  captureWorkspaceSession,
  isCurrentWorkspaceSession,
  requireWorkspaceSession,
  withWorkspaceSession,
  PostSchema,
  type ApiSession,
  type Post,
} from "./api";
import type {
  RetireScheduleRequest,
  SchedulePostRequest,
} from "./lib/api-contract.generated";

const ObservedPublication = z.object({
  channel_id: z.string().uuid(),
  publication_id: z.string().uuid(),
});
type ObservedPublication = z.infer<typeof ObservedPublication>;

// Keep the six fractional digits the server uses for request identity. JS Date
// alone truncates microseconds, so retain the final three digits separately.
export function canonicalScheduleTime(value: string): string | null {
  const parts =
    /^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(?:\.(\d{1,6}))?(Z|[+-]\d{2}:\d{2})$/i.exec(
      value,
    );
  if (!parts) return null;
  const fraction = (parts[2] ?? "").padEnd(6, "0");
  const local = Date.parse(`${parts[1]}Z`);
  if (
    !Number.isFinite(local) ||
    new Date(local).toISOString().slice(0, 19) !== parts[1]
  )
    return null;
  const ms = Date.parse(`${parts[1]}.${fraction.slice(0, 3)}${parts[3]}`);
  if (!Number.isFinite(ms)) return null;
  return `${new Date(ms).toISOString().slice(0, -1)}${fraction.slice(3)}Z`;
}

// Older records remain available for inspection but are not resent without an
// approved complete media-version map.
export const ScheduleIntent = z
  .object({
    id: z.string().uuid(),
    post: PostSchema,
    owner_id: z.string().uuid().optional(),
    brand_id: z.string().uuid().optional(),
    channel_ids: z.array(z.string().uuid()).min(1).max(50),
    mode: z.enum(["queue", "exact"]),
    scheduled_at: z.string().nullable().optional(),
    media_versions: z
      .record(z.string(), z.number().int().positive().safe())
      .optional(),
    request_contract: z.enum(["request-id-v1", "request-id-v2"]).optional(),
    // Local preference observed when the request was created. It is not part
    // of the server request, but fences a later correction against other tabs.
    correction_baseline: z.string().max(4096).nullable().optional(),
    uncertain: z.boolean(),
    rejection: z.string().optional(),
    rejection_code: z.string().optional(),
    observed_publication_ids: z.array(z.string().uuid()).optional(),
    observed_publications: z.array(ObservedPublication).max(1000).optional(),
    unverified_publication_ids: z.array(z.string().uuid()).max(1000).optional(),
    active_attempt_id: z.string().uuid().optional(),
  })
  .superRefine((intent, context) => {
    if (new Set(intent.channel_ids).size !== intent.channel_ids.length)
      context.addIssue({
        code: "custom",
        message: "Duplicate schedule channels",
      });
    if (
      intent.observed_publication_ids &&
      (intent.observed_publication_ids.length > 1000 ||
        new Set(intent.observed_publication_ids).size !==
          intent.observed_publication_ids.length ||
        intent.observed_publication_ids.some((id) => id !== id.toLowerCase()))
    )
      context.addIssue({
        code: "custom",
        message: "Invalid observed publication identities",
      });
    if (
      intent.observed_publications &&
      (new Set(
        intent.observed_publications.map(
          (row) => `${row.channel_id}:${row.publication_id}`,
        ),
      ).size !== intent.observed_publications.length ||
        intent.observed_publications.some(
          (row) =>
            row.channel_id !== row.channel_id.toLowerCase() ||
            row.publication_id !== row.publication_id.toLowerCase() ||
            !intent.channel_ids.includes(row.channel_id),
        ))
    )
      context.addIssue({
        code: "custom",
        message: "Invalid observed publication bindings",
      });
    if (
      intent.unverified_publication_ids &&
      (new Set(intent.unverified_publication_ids).size !==
        intent.unverified_publication_ids.length ||
        intent.unverified_publication_ids.some(
          (id) =>
            id !== id.toLowerCase() ||
            !intent.observed_publication_ids?.includes(id),
        ))
    )
      context.addIssue({
        code: "custom",
        message: "Invalid unverified publication identities",
      });
    if (
      (intent.mode === "exact" &&
        (!intent.scheduled_at ||
          !canonicalScheduleTime(intent.scheduled_at))) ||
      (intent.mode === "queue" && intent.scheduled_at != null)
    )
      context.addIssue({ code: "custom", message: "Invalid schedule time" });
    if (intent.request_contract === "request-id-v2") {
      const versions = intent.media_versions;
      if (
        !intent.owner_id ||
        intent.owner_id !== intent.owner_id.toLowerCase() ||
        !intent.brand_id ||
        intent.brand_id !== intent.brand_id.toLowerCase() ||
        intent.id !== intent.id.toLowerCase() ||
        intent.post.id !== intent.post.id.toLowerCase() ||
        intent.brand_id !== intent.post.brand_id ||
        !versions ||
        intent.channel_ids.some((id) => id !== id.toLowerCase()) ||
        intent.channel_ids.join(",") !==
          [...intent.channel_ids].sort().join(",") ||
        Object.keys(versions).length > 1000 ||
        Object.keys(versions).some(
          (id) =>
            id !== id.toLowerCase() || !z.string().uuid().safeParse(id).success,
        ) ||
        JSON.stringify(Object.keys(versions).sort()) !==
          JSON.stringify(effectiveMediaIds(intent.post, intent.channel_ids))
      )
        context.addIssue({
          code: "custom",
          message: "Invalid saved scheduling scope or media versions",
        });
    }
  });
export type ScheduleIntent = z.infer<typeof ScheduleIntent>;

const ScheduleCorrection = z
  .object({
    revision: z.string().uuid(),
    owner_id: z.string().uuid(),
    brand_id: z.string().uuid(),
    post_id: z.string().uuid(),
    mode: z.enum(["queue", "exact"]),
    scheduled_at: z.string().nullable(),
  })
  .strict()
  .refine((value) =>
    value.mode === "queue"
      ? value.scheduled_at === null
      : !!value.scheduled_at && !!canonicalScheduleTime(value.scheduled_at),
  );
export type ScheduleCorrection = z.infer<typeof ScheduleCorrection>;
export type CorrectionSnapshot = {
  key: string;
  raw: string | null;
  value: ScheduleCorrection | null;
  corrupt: boolean;
};
export const correctionEvent = "beweft-schedule-correction";
export class ScheduleCorrectionConflict extends Error {
  constructor() {
    super(
      "The saved scheduling time changed in another tab. Review both choices before continuing.",
    );
  }
}
export function readScheduleCorrection(
  owner: string,
  brand: string,
  post: string,
): CorrectionSnapshot {
  const key = `beweft.schedule-correction:${owner}:${brand}:${post}`;
  const raw = localStorage.getItem(key);
  if (raw === null) return { key, raw, value: null, corrupt: false };
  try {
    const value = ScheduleCorrection.parse(JSON.parse(raw));
    if (
      value.owner_id !== owner ||
      value.brand_id !== brand ||
      value.post_id !== post
    )
      throw new Error("Different scheduling scope");
    return { key, raw, value, corrupt: false };
  } catch {
    return { key, raw, value: null, corrupt: true };
  }
}
export function assertScheduleCorrection(snapshot: CorrectionSnapshot) {
  if (localStorage.getItem(snapshot.key) !== snapshot.raw)
    throw new Error(
      "The saved scheduling time changed in another tab. Reopen this draft to review it.",
    );
}
export function removeScheduleCorrection(
  snapshot: CorrectionSnapshot,
  session: ApiSession,
  signal?: AbortSignal,
  pendingKey?: string,
) {
  return withWorkspaceSession(
    session,
    () => {
      if (pendingKey && readScheduleIntent(pendingKey))
        throw new Error(
          "Another tab has an unfinished schedule. Review it before changing the saved time.",
        );
      assertScheduleCorrection(snapshot);
      if (snapshot.raw !== null) {
        localStorage.removeItem(snapshot.key);
        if (localStorage.getItem(snapshot.key) !== null)
          throw new Error("The saved scheduling time could not be updated.");
        window.dispatchEvent(new Event(correctionEvent));
      }
    },
    signal,
  );
}
function persistCorrection(intent: ScheduleIntent, key: string) {
  const preference: ScheduleCorrection = {
    revision: crypto.randomUUID(),
    owner_id: intent.owner_id!,
    brand_id: intent.brand_id!,
    post_id: intent.post.id,
    mode: intent.mode,
    scheduled_at: intent.scheduled_at ?? null,
  };
  const raw = JSON.stringify(preference);
  localStorage.setItem(key, raw);
  if (localStorage.getItem(key) !== raw)
    throw new Error(
      "The scheduling time could not be saved in browser storage.",
    );
  window.dispatchEvent(new Event(correctionEvent));
  return { key, raw, value: preference, corrupt: false };
}
function saveCorrectionForRejectedIntent(intent: ScheduleIntent) {
  if (!intent.owner_id || !intent.brand_id)
    throw new Error("The saved schedule has no verified owner or brand.");
  const existing = readScheduleCorrection(
    intent.owner_id,
    intent.brand_id,
    intent.post.id,
  );
  const sameChoice =
    existing.value?.mode === intent.mode &&
    existing.value.scheduled_at === (intent.scheduled_at ?? null);
  if (existing.corrupt) throw new ScheduleCorrectionConflict();
  // A newer tab may already have stored the same choice. Preserve its exact
  // record instead of replacing its revision while resolving this request.
  if (sameChoice) return existing;
  if (existing.raw !== (intent.correction_baseline ?? null))
    throw new ScheduleCorrectionConflict();
  return persistCorrection(intent, existing.key);
}
// This only changes the local preference after an explicit choice. The
// unresolved request remains intact and must still be checked or stopped.
export function chooseOriginalCorrection(
  pendingKey: string,
  intent: ScheduleIntent,
  snapshot: CorrectionSnapshot,
  session: ApiSession,
  signal?: AbortSignal,
) {
  return withWorkspaceSession(
    session,
    () => {
      const saved = readScheduleIntent(pendingKey);
      if (
        !saved ||
        !sameScheduleIntent(saved, intent) ||
        scheduleHasConflict(saved)
      )
        throw new Error(
          "The saved schedule changed. Reopen it before continuing.",
        );
      if (!saved.owner_id || !saved.brand_id)
        throw new Error("The saved schedule has no verified owner or brand.");
      const current = readScheduleCorrection(
        saved.owner_id,
        saved.brand_id,
        saved.post.id,
      );
      if (current.key !== snapshot.key || current.raw !== snapshot.raw)
        throw new ScheduleCorrectionConflict();
      return persistCorrection(saved, current.key);
    },
    signal,
  );
}
export const PublicationSummary = z.object({
  id: z.string().uuid(),
  post_id: z.string().uuid(),
  brand_id: z.string().uuid(),
  channel_id: z.string().uuid(),
  channel_name: z.string(),
  provider: z.string(),
  post_version: z.number(),
  scheduled_at: z.string(),
  scheduling_mode: z.enum(["queue", "exact"]),
  state: z.string(),
  created_at: z.string(),
  updated_at: z.string(),
});
export type PublicationSummary = z.infer<typeof PublicationSummary>;
const ScheduleReceipt = z.object({
  request_id: z.string().uuid(),
  post_id: z.string().uuid(),
  brand_id: z.string().uuid(),
  request: z.object({
    version: z.number(),
    channel_ids: z.array(z.string().uuid()),
    scheduled_at: z.string().nullable(),
    media_versions: z
      .record(z.string(), z.number().int().positive().safe())
      .nullable(),
  }),
  outcome: z.enum(["accepted", "rejected"]),
  error: z.object({ code: z.string(), message: z.string() }).nullable(),
  // Validate each row independently so a malformed sibling cannot erase
  // evidence of a publication the server already returned.
  publications: z.array(z.unknown()),
});
export const scheduleEvent = "beweft-schedule-intent";
export function readScheduleIntent(key: string) {
  const raw = localStorage.getItem(key);
  return raw === null ? null : ScheduleIntent.parse(JSON.parse(raw));
}
function persistScheduleIntent(key: string, intent: ScheduleIntent) {
  localStorage.setItem(key, JSON.stringify(intent));
  const stored = readScheduleIntent(key);
  if (
    !stored ||
    !sameScheduleIntent(stored, intent) ||
    stored.uncertain !== intent.uncertain ||
    stored.rejection !== intent.rejection ||
    stored.rejection_code !== intent.rejection_code ||
    stored.active_attempt_id !== intent.active_attempt_id ||
    JSON.stringify(stored.unverified_publication_ids) !==
      JSON.stringify(intent.unverified_publication_ids) ||
    JSON.stringify(stored.observed_publications) !==
      JSON.stringify(intent.observed_publications) ||
    JSON.stringify(stored.observed_publication_ids) !==
      JSON.stringify(intent.observed_publication_ids)
  )
    throw new Error(
      "The schedule could not be saved in browser storage. Check storage before continuing.",
    );
}
export function clearScheduleIntent(
  key: string,
  intent: ScheduleIntent,
  confirmed:
    | { kind: "accepted"; publications: PublicationSummary[] }
    | { kind: "rejected" },
  session: ApiSession,
  signal?: AbortSignal,
) {
  return withWorkspaceSession(
    session,
    () => {
      const saved = readScheduleIntent(key);
      if (!saved || !sameScheduleIntent(saved, intent))
        throw new Error(
          "The saved schedule changed. Reopen it before continuing.",
        );
      if (
        saved.rejection_code === "schedule_request_conflict" ||
        (confirmed.kind === "rejected"
          ? hasPublicationEvidence(saved)
          : !matchesObservedPublications(saved, confirmed.publications))
      ) {
        if (!scheduleHasConflict(saved)) {
          persistScheduleIntent(key, {
            ...saved,
            uncertain: true,
            rejection:
              "The server record does not match the observed publications.",
            rejection_code: "schedule_request_conflict",
          });
          window.dispatchEvent(new Event(scheduleEvent));
        }
        throw new Error(
          "The original scheduling outcome does not match the saved record. Keep it for review.",
        );
      }
      const correction =
        confirmed.kind === "rejected"
          ? saveCorrectionForRejectedIntent(saved)
          : null;
      localStorage.removeItem(key);
      if (readScheduleIntent(key))
        throw new Error(
          "The confirmed schedule could not be cleared from browser storage.",
        );
      window.dispatchEvent(new Event(scheduleEvent));
      return correction;
    },
    signal,
  );
}
export async function scheduleLock<T>(
  key: string,
  work: (session: ApiSession) => Promise<T>,
  signal?: AbortSignal,
  expectedSession?: ApiSession,
  releaseOnAbort = false,
) {
  if (!navigator.locks)
    throw new Error("Safe scheduling requires HTTPS or localhost.");
  const session = expectedSession ?? captureWorkspaceSession();
  requireWorkspaceSession(session);
  return navigator.locks.request(key, { signal }, async () => {
    signal?.throwIfAborted();
    requireWorkspaceSession(session);
    const operation = work(session);
    if (!releaseOnAbort || !signal) return operation;
    let cancel!: () => void;
    const abandoned = new Promise<never>((_resolve, reject) => {
      cancel = () =>
        reject(signal.reason ?? new DOMException("Abandoned", "AbortError"));
      if (signal.aborted) cancel();
      else signal.addEventListener("abort", cancel, { once: true });
    });
    try {
      return await Promise.race([operation, abandoned]);
    } finally {
      signal.removeEventListener("abort", cancel);
    }
  });
}
export function makeScheduleIntent(
  post: Post,
  owner: string,
  channelIds: string[],
  mode: "queue" | "exact",
  date: string,
  mediaVersions: Record<string, number>,
  preservedExact?: string | null,
  correctionBaseline: string | null = null,
): ScheduleIntent {
  return ScheduleIntent.parse({
    id: crypto.randomUUID(),
    post,
    owner_id: owner,
    brand_id: post.brand_id,
    channel_ids: [...new Set(channelIds)].sort(),
    mode,
    request_contract: "request-id-v2",
    correction_baseline: correctionBaseline,
    uncertain: false,
    scheduled_at:
      mode === "exact"
        ? (preservedExact ?? new Date(date).toISOString())
        : null,
    media_versions: Object.fromEntries(
      Object.entries(mediaVersions).sort(([a], [b]) => a.localeCompare(b)),
    ),
  });
}
export function effectiveMediaIds(post: Post, channelIds: string[]) {
  return [
    ...new Set(
      channelIds.flatMap((channel) => {
        const variant = post.variants.find(
          (item) => item.channel_id === channel,
        );
        return variant?.media_ids ?? post.media_ids;
      }),
    ),
  ].sort();
}
function sameMediaVersions(
  a: Record<string, number> | null | undefined,
  b: Record<string, number> | null | undefined,
) {
  if (a == null || b == null) return a == null && b == null;
  const keys = Object.keys(a).sort();
  return (
    keys.length === Object.keys(b).length && keys.every((id) => a[id] === b[id])
  );
}
export function sameScheduleIntent(a: ScheduleIntent, b: ScheduleIntent) {
  // Object property order in saved options is not part of the snapshot, but
  // array order and every saved field are. Canonicalize before comparing.
  const canonical = (value: unknown): unknown => {
    if (Array.isArray(value)) return value.map(canonical);
    if (value && typeof value === "object")
      return Object.fromEntries(
        Object.entries(value)
          .sort(([left], [right]) => left.localeCompare(right))
          .map(([key, item]) => [key, canonical(item)]),
      );
    return value;
  };
  return (
    a.id === b.id &&
    a.post.id === b.post.id &&
    a.post.brand_id === b.post.brand_id &&
    a.post.version === b.post.version &&
    JSON.stringify(canonical(a.post)) === JSON.stringify(canonical(b.post)) &&
    a.owner_id === b.owner_id &&
    a.brand_id === b.brand_id &&
    a.request_contract === b.request_contract &&
    a.correction_baseline === b.correction_baseline &&
    sameMediaVersions(a.media_versions, b.media_versions) &&
    a.mode === b.mode &&
    (a.scheduled_at ? canonicalScheduleTime(a.scheduled_at) : null) ===
      (b.scheduled_at ? canonicalScheduleTime(b.scheduled_at) : null) &&
    JSON.stringify([...a.channel_ids].sort()) ===
      JSON.stringify([...b.channel_ids].sort())
  );
}
export function scheduleHasConflict(intent: ScheduleIntent) {
  return intent.rejection_code === "schedule_request_conflict";
}
export function requireSavedSchedule(
  key: string,
  intent: ScheduleIntent,
  session: ApiSession,
  signal?: AbortSignal,
) {
  return withWorkspaceSession(
    session,
    () => {
      const saved = readScheduleIntent(key);
      if (!saved || !sameScheduleIntent(saved, intent))
        throw new Error(
          "The saved schedule changed. Reopen it before continuing.",
        );
      if (scheduleHasConflict(saved))
        throw new Error("The server record does not match this saved request.");
      return saved;
    },
    signal,
  );
}
export async function markScheduleMismatch(
  key: string,
  intent: ScheduleIntent,
  session: ApiSession,
) {
  await withWorkspaceSession(session, () => {
    const saved = readScheduleIntent(key);
    if (!saved || !sameScheduleIntent(saved, intent)) return;
    persistScheduleIntent(key, {
      ...saved,
      uncertain: true,
      rejection: "The server record does not match this saved request.",
      rejection_code: "schedule_request_conflict",
    });
    window.dispatchEvent(new Event(scheduleEvent));
  });
}
const PublicationBinding = z.object({
  id: z.string().uuid(),
  post_id: z.string().uuid(),
  brand_id: z.string().uuid(),
  channel_id: z.string().uuid(),
  post_version: z.number().int(),
});
const PublicationIdentity = z.object({ id: z.string().uuid() });
function hasPublicationEvidence(intent: ScheduleIntent) {
  return !!(
    intent.observed_publication_ids?.length ||
    intent.observed_publications?.length
  );
}
function unverifiedPublicationIds(intent: ScheduleIntent) {
  const bound = new Set(
    (intent.observed_publications ?? []).map((row) => row.publication_id),
  );
  return [
    ...new Set([
      ...(intent.unverified_publication_ids ?? []),
      ...(intent.observed_publication_ids ?? []).filter((id) => !bound.has(id)),
    ]),
  ].sort();
}
function analyzePublicationRows(intent: ScheduleIntent, rows: unknown[]) {
  const channels = new Set(intent.channel_ids);
  const ids = new Set<string>(),
    observedIds = new Set<string>(),
    seen = new Set<string>();
  const bindings: ObservedPublication[] = [];
  const publications: PublicationSummary[] = [];
  let mismatch = false,
    incomplete = false;
  for (const raw of rows) {
    const identity = PublicationIdentity.safeParse(raw);
    if (identity.success) observedIds.add(identity.data.id.toLowerCase());
    const result = PublicationBinding.safeParse(raw);
    if (!result.success) {
      incomplete = true;
      continue;
    }
    const row = result.data;
    const id = row.id.toLowerCase(),
      channel = row.channel_id.toLowerCase();
    if (
      !channels.has(channel) ||
      row.post_id.toLowerCase() !== intent.post.id ||
      row.brand_id.toLowerCase() !== intent.post.brand_id ||
      row.post_version !== intent.post.version
    ) {
      mismatch = true;
      continue;
    }
    bindings.push({ channel_id: channel, publication_id: id });
    if (ids.has(id) || seen.has(channel)) mismatch = true;
    ids.add(id);
    seen.add(channel);
    const summary = PublicationSummary.safeParse(raw);
    if (!summary.success) incomplete = true;
    else
      publications.push({
        ...summary.data,
        id,
        channel_id: channel,
        post_id: row.post_id.toLowerCase(),
        brand_id: row.brand_id.toLowerCase(),
      });
  }
  return {
    ids: [...observedIds],
    bindings,
    publications,
    mismatch,
    complete: !mismatch && !incomplete && seen.size === channels.size,
  };
}
function matchesObservedPublications(
  intent: ScheduleIntent,
  rows: PublicationSummary[],
) {
  const analyzed = analyzePublicationRows(intent, rows);
  if (!analyzed.complete || unverifiedPublicationIds(intent).length)
    return false;
  const byChannel = new Map(
    analyzed.bindings.map((row) => [row.channel_id, row.publication_id]),
  );
  const ids = new Set(analyzed.bindings.map((row) => row.publication_id));
  return (
    (intent.observed_publication_ids ?? []).every((id) => ids.has(id)) &&
    (intent.observed_publications ?? []).every(
      (row) => byChannel.get(row.channel_id) === row.publication_id,
    )
  );
}
async function recordPublicationObservations(
  key: string,
  intent: ScheduleIntent,
  rows: unknown[],
  session: ApiSession,
  verifiedId?: string,
) {
  const analyzed = analyzePublicationRows(intent, rows);
  return withWorkspaceSession(session, () => {
    const saved = readScheduleIntent(key);
    if (!saved || !sameScheduleIntent(saved, intent))
      throw new Error(
        "The saved schedule changed. Reopen it before continuing.",
      );
    const previous = saved.observed_publications ?? [];
    const legacyIds = saved.observed_publication_ids ?? [];
    const unverified = new Set(unverifiedPublicationIds(saved));
    const bindings = [...previous];
    const keys = new Set(
      previous.map((row) => `${row.channel_id}:${row.publication_id}`),
    );
    const byChannel = new Map<string, string>(),
      byId = new Map<string, string>();
    let mismatch = analyzed.mismatch || scheduleHasConflict(saved);
    for (const row of previous) {
      if (
        (byChannel.has(row.channel_id) &&
          byChannel.get(row.channel_id) !== row.publication_id) ||
        (byId.has(row.publication_id) &&
          byId.get(row.publication_id) !== row.channel_id)
      )
        mismatch = true;
      byChannel.set(row.channel_id, row.publication_id);
      byId.set(row.publication_id, row.channel_id);
    }
    for (const row of analyzed.bindings) {
      if (
        (byChannel.has(row.channel_id) &&
          byChannel.get(row.channel_id) !== row.publication_id) ||
        (byId.has(row.publication_id) &&
          byId.get(row.publication_id) !== row.channel_id) ||
        (legacyIds.length >= saved.channel_ids.length &&
          !legacyIds.includes(row.publication_id))
      )
        mismatch = true;
      const pair = `${row.channel_id}:${row.publication_id}`;
      if (!keys.has(pair)) {
        if (bindings.length >= 1000) mismatch = true;
        else {
          bindings.push(row);
          keys.add(pair);
        }
      }
      byChannel.set(row.channel_id, row.publication_id);
      byId.set(row.publication_id, row.channel_id);
    }
    if (
      verifiedId &&
      !mismatch &&
      analyzed.bindings.length === 1 &&
      analyzed.bindings[0].publication_id === verifiedId
    )
      unverified.delete(verifiedId);
    bindings.sort(
      (a, b) =>
        a.channel_id.localeCompare(b.channel_id) ||
        a.publication_id.localeCompare(b.publication_id),
    );
    const ids = [...legacyIds];
    for (const id of analyzed.ids)
      if (!ids.includes(id)) {
        if (ids.length >= 1000) mismatch = true;
        else ids.push(id);
      }
    ids.sort();
    const boundIds = new Set(bindings.map((row) => row.publication_id));
    for (const id of ids) if (!boundIds.has(id)) unverified.add(id);
    const updated: ScheduleIntent = {
      ...saved,
      ...(bindings.length ? { observed_publications: bindings } : {}),
      ...(ids.length ? { observed_publication_ids: ids } : {}),
      unverified_publication_ids: unverified.size
        ? [...unverified].sort()
        : undefined,
      ...(mismatch
        ? {
            uncertain: true,
            rejection:
              "The server record does not match the observed publications.",
            rejection_code: "schedule_request_conflict",
          }
        : {}),
    };
    persistScheduleIntent(key, updated);
    window.dispatchEvent(new Event(scheduleEvent));
    return { ...analyzed, mismatch, saved: updated };
  });
}
// Earlier clients saved only publication IDs. Check their channel bindings
// against independent owner-scoped summaries before trusting a receipt.
async function verifyLegacyPublicationBindings(
  key: string,
  intent: ScheduleIntent,
  session: ApiSession,
  signal?: AbortSignal,
) {
  const saved = await requireSavedSchedule(key, intent, session, signal);
  const ids = unverifiedPublicationIds(saved);
  for (let offset = 0; offset < ids.length; offset += 5) {
    await Promise.all(
      ids.slice(offset, offset + 5).map(async (id) => {
        const rows = await api(
          `/publications?brand_id=${encodeURIComponent(intent.post.brand_id)}&id=${encodeURIComponent(id)}&view=summary`,
          z.array(z.unknown()),
          "GET",
          undefined,
          signal,
        );
        const analyzed = analyzePublicationRows(intent, rows);
        if (
          rows.length !== 1 ||
          analyzed.bindings.length !== 1 ||
          analyzed.bindings[0].publication_id !== id ||
          analyzed.mismatch
        ) {
          if (analyzed.bindings.length)
            await recordPublicationObservations(key, intent, rows, session);
          throw new Error(
            "An earlier publication could not be verified. Check the original schedule again.",
          );
        }
        const observed = await recordPublicationObservations(
          key,
          intent,
          rows,
          session,
          id,
        );
        if (observed.mismatch)
          throw new ApiError(
            "The server record does not match the observed publications.",
            409,
            "schedule_request_conflict",
          );
      }),
    );
  }
}
export async function sendScheduleIntent(
  key: string,
  intent: ScheduleIntent,
  session: ApiSession,
  retry = false,
  signal?: AbortSignal,
) {
  if (intent.request_contract !== "request-id-v2" || !intent.media_versions)
    throw new Error(
      "This older schedule was sent without a request ID and cannot be retried safely.",
    );
  // Claim and persist uncertainty under both locks before crossing the network.
  const attemptId = crypto.randomUUID();
  await withWorkspaceSession(
    session,
    () => {
      const saved = readScheduleIntent(key);
      if (retry ? !saved || !sameScheduleIntent(saved, intent) : !!saved)
        throw new Error(
          "The saved scheduling intent changed. Reopen it before continuing.",
        );
      if (retry && saved && scheduleHasConflict(saved))
        throw new ApiError(
          "The server record does not match this saved request.",
          409,
          "schedule_request_conflict",
        );
      if (!retry) {
        const correction = readScheduleCorrection(
          intent.owner_id!,
          intent.brand_id!,
          intent.post.id,
        );
        if (
          correction.corrupt ||
          correction.raw !== (intent.correction_baseline ?? null)
        )
          throw new Error(
            "The saved scheduling time changed in another tab. Reopen this draft to review it.",
          );
      }
      const priorIntent = saved ?? intent;
      persistScheduleIntent(key, {
        ...priorIntent,
        uncertain: true,
        rejection: undefined,
        rejection_code: undefined,
        active_attempt_id: attemptId,
      });
      window.dispatchEvent(new Event(scheduleEvent));
    },
    signal,
  );
  let requestSucceeded = false;
  try {
    signal?.throwIfAborted();
    requireWorkspaceSession(session);
    const rows = await api(
      `/posts/${intent.post.id}/schedule?view=summary`,
      z.array(z.unknown()),
      "POST",
      {
        request_id: intent.id,
        version: intent.post.version,
        channel_ids: intent.channel_ids,
        scheduled_at: intent.scheduled_at ?? null,
        media_versions: intent.media_versions,
      } satisfies SchedulePostRequest & { request_id: string },
    );
    requestSucceeded = true;
    requireWorkspaceSession(session);
    const observed = await recordPublicationObservations(
      key,
      intent,
      rows,
      session,
    );
    if (observed.mismatch) {
      throw new ApiError(
        "The scheduling response did not match the original request.",
        409,
        "schedule_request_conflict",
      );
    }
    if (!observed.complete)
      throw new Error(
        "The scheduling response was incomplete or did not match this request.",
      );
    // A publication may be rescheduled or canceled later, so its current
    // summary cannot establish the original time or media versions. Verify the
    // immutable receipt and require the same original identities before clearing.
    const receipt = await inspectScheduleIntent(
      key,
      observed.saved,
      session,
      signal,
    );
    if (receipt.kind === "mismatch") {
      await markScheduleMismatch(key, intent, session);
      throw new ApiError(
        "The server record does not match this saved request.",
        409,
        "schedule_request_conflict",
      );
    }
    if (receipt.kind !== "complete")
      throw new Error(
        "The scheduling outcome could not be fully confirmed. Check the original schedule again.",
      );
    return receipt.publications;
  } catch (failure) {
    if (isCurrentWorkspaceSession(session)) {
      try {
        await withWorkspaceSession(session, () => {
          const saved = readScheduleIntent(key);
          if (saved && sameScheduleIntent(saved, intent)) {
            const mismatch =
              failure instanceof ApiError &&
              failure.code === "schedule_request_conflict";
            const mediaConflict =
              failure instanceof ApiError &&
              !requestSucceeded &&
              failure.status === 409 &&
              failure.code === "media_version_conflict";
            // A proven mismatch dominates every later observation. A stale
            // abandoned transient may not overwrite a newer attempt's state.
            // A gated media conflict is stronger than an in-progress result;
            // retirement still verifies the durable outcome before clearing.
            if (
              scheduleHasConflict(saved) ||
              (!mismatch &&
                !mediaConflict &&
                saved.active_attempt_id !== attemptId)
            )
              return;
            if (mediaConflict && hasPublicationEvidence(saved)) {
              persistScheduleIntent(key, {
                ...saved,
                uncertain: true,
                rejection:
                  "The server record does not match the observed publications.",
                rejection_code: "schedule_request_conflict",
              });
              window.dispatchEvent(new Event(scheduleEvent));
              return;
            }
            persistScheduleIntent(key, {
              ...saved,
              uncertain: !mediaConflict,
              rejection:
                failure instanceof Error
                  ? failure.message
                  : "Scheduling outcome is unknown",
              rejection_code:
                failure instanceof ApiError ? failure.code : undefined,
            });
            window.dispatchEvent(new Event(scheduleEvent));
          }
        });
      } catch {
        // Keep the pre-send uncertain record when its outcome cannot be updated.
      }
    }
    throw failure;
  }
}
export async function inspectScheduleIntent(
  key: string,
  intent: ScheduleIntent,
  session: ApiSession,
  signal?: AbortSignal,
) {
  if (!intent.request_contract) return { kind: "legacy" as const };
  requireWorkspaceSession(session);
  await verifyLegacyPublicationBindings(key, intent, session, signal);
  let receipt: unknown;
  try {
    receipt = await api(
      `/schedule-requests/${intent.id}`,
      z.unknown(),
      "GET",
      undefined,
      signal,
    );
  } catch (failure) {
    if (failure instanceof ApiError && failure.status === 404) {
      requireWorkspaceSession(session);
      return { kind: "missing" as const };
    }
    throw failure;
  }
  requireWorkspaceSession(session);
  return matchScheduleReceipt(key, intent, receipt, session);
}

async function matchScheduleReceipt(
  key: string,
  intent: ScheduleIntent,
  rawReceipt: unknown,
  session: ApiSession,
) {
  const rows = z
    .object({ publications: z.array(z.unknown()) })
    .safeParse(rawReceipt);
  if (!rows.success)
    throw new Error("The scheduling receipt was incomplete. Check it again.");
  const observed = await recordPublicationObservations(
    key,
    intent,
    rows.data.publications,
    session,
  );
  const parsed = ScheduleReceipt.safeParse(rawReceipt);
  if (!parsed.success) {
    if (observed.mismatch)
      throw new ApiError(
        "The server record does not match the observed publications.",
        409,
        "schedule_request_conflict",
      );
    throw new Error("The scheduling receipt was incomplete. Check it again.");
  }
  const receipt = parsed.data;
  const requested = [...intent.channel_ids].sort();
  const returned = [...receipt.request.channel_ids].sort();
  const expectedTime = intent.scheduled_at
    ? canonicalScheduleTime(intent.scheduled_at)
    : null;
  const returnedTime = receipt.request.scheduled_at
    ? canonicalScheduleTime(receipt.request.scheduled_at)
    : null;
  if (
    receipt.request_id !== intent.id ||
    receipt.post_id !== intent.post.id ||
    receipt.brand_id !== intent.post.brand_id ||
    receipt.request.version !== intent.post.version ||
    JSON.stringify(requested) !== JSON.stringify(returned) ||
    !sameMediaVersions(
      intent.media_versions ?? null,
      receipt.request.media_versions,
    ) ||
    expectedTime !== returnedTime ||
    (!!receipt.request.scheduled_at && returnedTime === null) ||
    observed.mismatch
  ) {
    await markScheduleMismatch(key, intent, session);
    return { kind: "mismatch" as const };
  }
  if (receipt.outcome === "rejected") {
    if (
      !receipt.error ||
      receipt.publications.length !== 0 ||
      hasPublicationEvidence(observed.saved)
    ) {
      await markScheduleMismatch(key, intent, session);
      return { kind: "mismatch" as const };
    }
    return { kind: "rejected" as const, error: receipt.error };
  }
  if (unverifiedPublicationIds(observed.saved).length)
    return { kind: "partial" as const, publications: observed.publications };
  if (
    receipt.error ||
    (observed.complete &&
      !matchesObservedPublications(observed.saved, observed.publications))
  ) {
    await markScheduleMismatch(key, intent, session);
    return { kind: "mismatch" as const };
  }
  return {
    kind: observed.complete ? ("complete" as const) : ("partial" as const),
    publications: observed.publications,
  };
}

// The server serializes retirement with the original request. A successful
// response either returns its existing result or prevents a late original POST
// from ever accepting. Missing records and transport errors are inconclusive.
export async function retireScheduleIntent(
  key: string,
  intent: ScheduleIntent,
  session: ApiSession,
  signal?: AbortSignal,
) {
  if (intent.request_contract !== "request-id-v2" || !intent.media_versions)
    throw new Error("This older schedule cannot be stopped safely here.");
  signal?.throwIfAborted();
  requireWorkspaceSession(session);
  await verifyLegacyPublicationBindings(key, intent, session, signal);
  const receipt = await api(
    `/schedule-requests/${intent.id}/retire`,
    z.unknown(),
    "POST",
    {
      post_id: intent.post.id,
      brand_id: intent.post.brand_id,
      version: intent.post.version,
      channel_ids: intent.channel_ids,
      scheduled_at: intent.scheduled_at ?? null,
      media_versions: intent.media_versions,
    } satisfies RetireScheduleRequest,
  );
  requireWorkspaceSession(session);
  const result = await matchScheduleReceipt(key, intent, receipt, session);
  signal?.throwIfAborted();
  return result;
}
