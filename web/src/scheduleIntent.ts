import { z } from "zod";
import { api, ApiError, PostSchema, PublicationSchema, type Post } from "./api";
import type { SchedulePostRequest } from "./lib/api-contract.generated";
export const ScheduleIntent = z.object({
  id: z.string().uuid(),
  post: PostSchema,
  channel_ids: z.array(z.string()),
  mode: z.enum(["queue", "exact"]),
  scheduled_at: z.string().optional(),
  uncertain: z.boolean(),
  rejection: z.string().optional(),
});
export type ScheduleIntent = z.infer<typeof ScheduleIntent>;
export const scheduleEvent = "beweft-schedule-intent";
export function readScheduleIntent(key: string) {
  const raw = localStorage.getItem(key);
  return raw ? ScheduleIntent.parse(JSON.parse(raw)) : null;
}
export function writeScheduleIntent(key: string, intent: ScheduleIntent) {
  localStorage.setItem(key, JSON.stringify(intent));
  window.dispatchEvent(new Event(scheduleEvent));
}
export function clearScheduleIntent(key: string, id: string) {
  if (readScheduleIntent(key)?.id === id) localStorage.removeItem(key);
  window.dispatchEvent(new Event(scheduleEvent));
}
export async function scheduleLock<T>(key: string, work: () => Promise<T>) {
  if (!navigator.locks)
    throw new Error("Safe scheduling requires HTTPS or localhost.");
  return navigator.locks.request(key, work);
}
export function makeScheduleIntent(
  post: Post,
  channel_ids: string[],
  mode: "queue" | "exact",
  date: string,
): ScheduleIntent {
  return {
    id: crypto.randomUUID(),
    post,
    channel_ids,
    mode,
    uncertain: false,
    ...(mode === "exact" ? { scheduled_at: new Date(date).toISOString() } : {}),
  };
}
export async function sendScheduleIntent(key: string, intent: ScheduleIntent) {
  // Persist uncertainty before crossing the network so reload cannot erase it.
  writeScheduleIntent(key, {
    ...intent,
    uncertain: true,
    rejection: undefined,
  });
  try {
    return await api(
      `/posts/${intent.post.id}/schedule`,
      z.array(PublicationSchema),
      "POST",
      {
        version: intent.post.version,
        channel_ids: intent.channel_ids,
        ...(intent.scheduled_at ? { scheduled_at: intent.scheduled_at } : {}),
      } satisfies SchedulePostRequest,
    );
  } catch (failure) {
    if (readScheduleIntent(key)?.id === intent.id)
      writeScheduleIntent(key, {
        ...intent,
        uncertain:
          intent.uncertain ||
          !(
            failure instanceof ApiError &&
            failure.status >= 400 &&
            failure.status < 500
          ),
        rejection:
          failure instanceof Error
            ? failure.message
            : "Scheduling outcome is unknown",
      });
    throw failure;
  }
}
export async function inspectScheduleIntent(intent: ScheduleIntent) {
  // Read the version first: a later publication read must include any commit
  // that preceded a version change. Parallel reads cannot establish that fence.
  const post = await api(`/posts/${intent.post.id}`, PostSchema);
  const publications = await api(
    `/publications?brand_id=${intent.post.brand_id}`,
    z.array(PublicationSchema),
  );
  if (post.brand_id !== intent.post.brand_id)
    throw new Error("Post belongs to a different brand; recovery stopped.");
  const recorded = publications.filter(
    (p) =>
      p.post_id === intent.post.id &&
      p.post_version === intent.post.version &&
      intent.channel_ids.includes(p.channel_id),
  );
  const complete = intent.channel_ids.every((id) =>
    recorded.some((p) => p.channel_id === id),
  );
  return {
    post,
    recorded,
    complete,
    fenced: post.version !== intent.post.version,
    rejected: !!intent.rejection && !intent.uncertain && recorded.length === 0,
  };
}
