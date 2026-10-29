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
    uncertain: z.boolean(),
    rejection: z.string().optional(),
    rejection_code: z.string().optional(),
    observed_publication_ids: z.array(z.string().uuid()).optional(),
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
      (intent.observed_publication_ids.length !== intent.channel_ids.length ||
        new Set(intent.observed_publication_ids).size !==
          intent.observed_publication_ids.length ||
        intent.observed_publication_ids.some((id) => id !== id.toLowerCase()))
    )
      context.addIssue({
        code: "custom",
        message: "Invalid observed publication identities",
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
  publications: z.array(PublicationSummary),
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
    | { kind: "accepted"; publicationIds: string[] }
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
        (saved.observed_publication_ids &&
          (confirmed.kind !== "accepted" ||
            JSON.stringify([...saved.observed_publication_ids].sort()) !==
              JSON.stringify([...confirmed.publicationIds].sort())))
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
      localStorage.removeItem(key);
      if (readScheduleIntent(key))
        throw new Error(
          "The confirmed schedule could not be cleared from browser storage.",
        );
      window.dispatchEvent(new Event(scheduleEvent));
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
): ScheduleIntent {
  return ScheduleIntent.parse({
    id: crypto.randomUUID(),
    post,
    owner_id: owner,
    brand_id: post.brand_id,
    channel_ids: [...new Set(channelIds)].sort(),
    mode,
    request_contract: "request-id-v2",
    uncertain: false,
    scheduled_at: mode === "exact" ? new Date(date).toISOString() : null,
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
function publicationCoverage(
  intent: ScheduleIntent,
  rows: PublicationSummary[],
) {
  const channels = new Set(intent.channel_ids);
  const ids = new Set<string>(),
    seen = new Set<string>();
  for (const row of rows) {
    if (
      ids.has(row.id) ||
      seen.has(row.channel_id) ||
      !channels.has(row.channel_id) ||
      row.post_id !== intent.post.id ||
      row.brand_id !== intent.post.brand_id ||
      row.post_version !== intent.post.version
    )
      return "mismatch" as const;
    ids.add(row.id);
    seen.add(row.channel_id);
  }
  return seen.size === channels.size
    ? ("complete" as const)
    : ("partial" as const);
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
      z.array(PublicationSummary),
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
    const coverage = publicationCoverage(intent, rows);
    if (coverage === "mismatch") {
      await markScheduleMismatch(key, intent, session);
      throw new ApiError(
        "The scheduling response did not match the original request.",
        409,
        "schedule_request_conflict",
      );
    }
    if (coverage !== "complete")
      throw new Error(
        "The scheduling response was incomplete or did not match this request.",
      );
    const observed = await withWorkspaceSession(
      session,
      () => {
        const saved = readScheduleIntent(key);
        if (!saved || !sameScheduleIntent(saved, intent))
          throw new Error(
            "The saved schedule changed. Reopen it before continuing.",
          );
        if (scheduleHasConflict(saved))
          throw new ApiError(
            "The server record does not match this saved request.",
            409,
            "schedule_request_conflict",
          );
        const ids = rows.map((row) => row.id).sort();
        if (
          saved.observed_publication_ids &&
          JSON.stringify([...saved.observed_publication_ids].sort()) !==
            JSON.stringify(ids)
        )
          throw new ApiError(
            "The scheduling response did not match the original publications.",
            409,
            "schedule_request_conflict",
          );
        const updated = { ...saved, observed_publication_ids: ids };
        persistScheduleIntent(key, updated);
        window.dispatchEvent(new Event(scheduleEvent));
        return updated;
      },
      signal,
    );
    // A publication may be rescheduled or canceled later, so its current
    // summary cannot establish the original time or media versions. Verify the
    // immutable receipt and require the same original identities before clearing.
    const receipt = await inspectScheduleIntent(observed, session, signal);
    if (receipt.kind === "mismatch") {
      await markScheduleMismatch(key, intent, session);
      throw new ApiError(
        "The server record does not match this saved request.",
        409,
        "schedule_request_conflict",
      );
    }
    if (
      receipt.kind !== "complete" ||
      JSON.stringify(rows.map((row) => row.id).sort()) !==
        JSON.stringify(receipt.publications.map((row) => row.id).sort())
    )
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
            // A proven mismatch dominates every later observation. A stale
            // abandoned attempt may not overwrite a newer attempt's state.
            if (
              scheduleHasConflict(saved) ||
              (!mismatch && saved.active_attempt_id !== attemptId)
            )
              return;
            const confirmedRejection =
              failure instanceof ApiError &&
              !requestSucceeded &&
              failure.status === 409 &&
              failure.code === "media_version_conflict";
            persistScheduleIntent(key, {
              ...saved,
              uncertain: !confirmedRejection,
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
  intent: ScheduleIntent,
  session: ApiSession,
  signal?: AbortSignal,
) {
  if (!intent.request_contract) return { kind: "legacy" as const };
  requireWorkspaceSession(session);
  let receipt: z.infer<typeof ScheduleReceipt>;
  try {
    receipt = await api(
      `/schedule-requests/${intent.id}`,
      ScheduleReceipt,
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
  return matchScheduleReceipt(intent, receipt);
}

function matchScheduleReceipt(
  intent: ScheduleIntent,
  receipt: z.infer<typeof ScheduleReceipt>,
) {
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
    (!!receipt.request.scheduled_at && returnedTime === null)
  )
    return { kind: "mismatch" as const };
  if (receipt.outcome === "rejected") {
    if (
      !receipt.error ||
      receipt.publications.length !== 0 ||
      intent.observed_publication_ids
    )
      return { kind: "mismatch" as const };
    return { kind: "rejected" as const, error: receipt.error };
  }
  if (receipt.error) return { kind: "mismatch" as const };
  const coverage = publicationCoverage(intent, receipt.publications);
  if (
    coverage === "complete" &&
    intent.observed_publication_ids &&
    JSON.stringify([...intent.observed_publication_ids].sort()) !==
      JSON.stringify(receipt.publications.map((row) => row.id).sort())
  )
    return { kind: "mismatch" as const };
  return { kind: coverage, publications: receipt.publications };
}

// The server serializes retirement with the original request. A successful
// response either returns its existing result or prevents a late original POST
// from ever accepting. Missing records and transport errors are inconclusive.
export async function retireScheduleIntent(
  intent: ScheduleIntent,
  session: ApiSession,
  signal?: AbortSignal,
) {
  if (intent.request_contract !== "request-id-v2" || !intent.media_versions)
    throw new Error("This older schedule cannot be stopped safely here.");
  signal?.throwIfAborted();
  requireWorkspaceSession(session);
  const receipt = await api(
    `/schedule-requests/${intent.id}/retire`,
    ScheduleReceipt,
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
  signal?.throwIfAborted();
  requireWorkspaceSession(session);
  return matchScheduleReceipt(intent, receipt);
}
