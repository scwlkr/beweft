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
import type { SchedulePostRequest } from "./lib/api-contract.generated";

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
  })
  .superRefine((intent, context) => {
    if (new Set(intent.channel_ids).size !== intent.channel_ids.length)
      context.addIssue({
        code: "custom",
        message: "Duplicate schedule channels",
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
  publications: z.array(PublicationSummary),
});
export const scheduleEvent = "beweft-schedule-intent";
export function readScheduleIntent(key: string) {
  const raw = localStorage.getItem(key);
  return raw === null ? null : ScheduleIntent.parse(JSON.parse(raw));
}
export function writeScheduleIntent(
  key: string,
  intent: ScheduleIntent,
  session: ApiSession,
  signal?: AbortSignal,
) {
  return withWorkspaceSession(
    session,
    () => {
      localStorage.setItem(key, JSON.stringify(intent));
      window.dispatchEvent(new Event(scheduleEvent));
    },
    signal,
  );
}
export function clearScheduleIntent(
  key: string,
  id: string,
  session: ApiSession,
  signal?: AbortSignal,
) {
  return withWorkspaceSession(
    session,
    () => {
      if (readScheduleIntent(key)?.id === id) localStorage.removeItem(key);
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
  return (
    a.id === b.id &&
    a.post.id === b.post.id &&
    a.post.brand_id === b.post.brand_id &&
    a.post.version === b.post.version &&
    JSON.stringify(a.post) === JSON.stringify(b.post) &&
    a.owner_id === b.owner_id &&
    a.brand_id === b.brand_id &&
    a.request_contract === b.request_contract &&
    sameMediaVersions(a.media_versions, b.media_versions) &&
    a.mode === b.mode &&
    a.scheduled_at === b.scheduled_at &&
    JSON.stringify([...a.channel_ids].sort()) ===
      JSON.stringify([...b.channel_ids].sort())
  );
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
      row.post_version !== intent.post.version ||
      row.scheduling_mode !== intent.mode ||
      (intent.mode === "exact" &&
        canonicalScheduleTime(row.scheduled_at) !==
          canonicalScheduleTime(intent.scheduled_at!))
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
  const prior = await withWorkspaceSession(
    session,
    () => {
      const saved = readScheduleIntent(key);
      if (retry ? !saved || !sameScheduleIntent(saved, intent) : !!saved)
        throw new Error(
          "The saved scheduling intent changed. Reopen it before continuing.",
        );
      if (retry && saved?.rejection_code === "schedule_request_conflict")
        throw new ApiError(
          "The server record does not match this saved request.",
          409,
          "schedule_request_conflict",
        );
      const priorIntent = saved ?? intent;
      localStorage.setItem(
        key,
        JSON.stringify({
          ...priorIntent,
          uncertain: true,
          rejection: undefined,
          rejection_code: undefined,
        }),
      );
      window.dispatchEvent(new Event(scheduleEvent));
      return priorIntent;
    },
    signal,
  );
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
    requireWorkspaceSession(session);
    if (publicationCoverage(intent, rows) !== "complete")
      throw new Error(
        "The scheduling response was incomplete or did not match this request.",
      );
    return rows;
  } catch (failure) {
    if (isCurrentWorkspaceSession(session)) {
      try {
        await withWorkspaceSession(session, () => {
          const saved = readScheduleIntent(key);
          if (saved && sameScheduleIntent(saved, intent)) {
            const confirmedRejection =
              failure instanceof ApiError &&
              ((failure.status === 409 &&
                failure.code === "media_version_conflict") ||
                (!prior.uncertain &&
                  !signal?.aborted &&
                  failure.status >= 400 &&
                  failure.status < 500 &&
                  ![
                    "session_changed",
                    "schedule_request_in_progress",
                    "schedule_request_conflict",
                  ].includes(failure.code)));
            localStorage.setItem(
              key,
              JSON.stringify({
                ...saved,
                uncertain: !confirmedRejection,
                rejection:
                  failure instanceof Error
                    ? failure.message
                    : "Scheduling outcome is unknown",
                rejection_code:
                  failure instanceof ApiError ? failure.code : undefined,
              }),
            );
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
  const coverage = publicationCoverage(intent, receipt.publications);
  return { kind: coverage, publications: receipt.publications };
}
