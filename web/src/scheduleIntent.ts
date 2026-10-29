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

// Older saved intents were sent without an idempotency key and cannot safely
// be retried. Only intents explicitly marked with this contract may be replayed.
export const ScheduleIntent = z
  .object({
    id: z.string().uuid(),
    post: PostSchema,
    channel_ids: z.array(z.string().uuid()).min(1).max(50),
    mode: z.enum(["queue", "exact"]),
    scheduled_at: z.string().optional(),
    request_contract: z.literal("request-id-v1").optional(),
    uncertain: z.boolean(),
    rejection: z.string().optional(),
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
          !Number.isFinite(Date.parse(intent.scheduled_at)))) ||
      (intent.mode === "queue" && intent.scheduled_at !== undefined)
    )
      context.addIssue({ code: "custom", message: "Invalid schedule time" });
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
  }),
  publications: z.array(PublicationSummary),
});
export const scheduleEvent = "beweft-schedule-intent";
export function readScheduleIntent(key: string) {
  const raw = localStorage.getItem(key);
  return raw ? ScheduleIntent.parse(JSON.parse(raw)) : null;
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
  channelIds: string[],
  mode: "queue" | "exact",
  date: string,
): ScheduleIntent {
  return {
    id: crypto.randomUUID(),
    post,
    channel_ids: [...new Set(channelIds)].sort(),
    mode,
    request_contract: "request-id-v1",
    uncertain: false,
    ...(mode === "exact" ? { scheduled_at: new Date(date).toISOString() } : {}),
  };
}
export function sameScheduleIntent(a: ScheduleIntent, b: ScheduleIntent) {
  return (
    a.id === b.id &&
    a.post.id === b.post.id &&
    a.post.brand_id === b.post.brand_id &&
    a.post.version === b.post.version &&
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
  if (intent.request_contract !== "request-id-v1")
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
      const priorIntent = saved ?? intent;
      localStorage.setItem(
        key,
        JSON.stringify({
          ...priorIntent,
          uncertain: true,
          rejection: undefined,
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
        ...(intent.scheduled_at ? { scheduled_at: intent.scheduled_at } : {}),
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
              !prior.uncertain &&
              !signal?.aborted &&
              failure instanceof ApiError &&
              failure.status >= 400 &&
              failure.status < 500 &&
              failure.code !== "session_changed";
            localStorage.setItem(
              key,
              JSON.stringify({
                ...saved,
                uncertain: !confirmedRejection,
                rejection:
                  failure instanceof Error
                    ? failure.message
                    : "Scheduling outcome is unknown",
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
  if (intent.request_contract !== "request-id-v1")
    return { kind: "legacy" as const };
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
    ? Date.parse(intent.scheduled_at)
    : null;
  const returnedTime = receipt.request.scheduled_at
    ? Date.parse(receipt.request.scheduled_at)
    : null;
  if (
    receipt.request_id !== intent.id ||
    receipt.post_id !== intent.post.id ||
    receipt.brand_id !== intent.post.brand_id ||
    receipt.request.version !== intent.post.version ||
    JSON.stringify(requested) !== JSON.stringify(returned) ||
    expectedTime !== returnedTime ||
    (returnedTime !== null && !Number.isFinite(returnedTime))
  )
    return { kind: "mismatch" as const };
  const coverage = publicationCoverage(intent, receipt.publications);
  return { kind: coverage, publications: receipt.publications };
}
