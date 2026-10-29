import { z } from "zod";
const id = z.string();
export const BrandSchema = z.object({
  id,
  name: z.string(),
  color: z.string(),
  created_at: z.string(),
});
export const ChannelSchema = z
  .object({
    id,
    brand_id: id,
    name: z.string(),
    provider: z.string(),
    timezone: z.string(),
    paused: z.boolean(),
    capabilities: z.unknown(),
    created_at: z.string(),
  })
  .passthrough();
export const IdeaSchema = z.object({
  id,
  brand_id: id,
  title: z.string(),
  text: z.string(),
  color: z.string(),
  created_at: z.string(),
  updated_at: z.string(),
});
export const VariantSchema = z.object({
  channel_id: id,
  options: z.record(z.string(), z.unknown()).optional(),
  text: z.string().optional(),
  media_ids: z.array(id).optional(),
});
export const PostSchema = z.object({
  id,
  brand_id: id,
  title: z.string(),
  text: z.string(),
  media_ids: z.array(id),
  variants: z.array(VariantSchema),
  options: z.record(z.string(), z.unknown()).optional(),
  version: z.number(),
  status: z.string(),
  created_at: z.string(),
  updated_at: z.string(),
});
export const MediaSchema = z.object({
  id,
  brand_id: id,
  name: z.string(),
  mime: z.string(),
  size: z.number(),
  url: z.string(),
  created_at: z.string(),
});
export const PublicationSchema = z.object({
  id,
  post_id: id,
  brand_id: id,
  channel_id: id,
  channel_name: z.string(),
  provider: z.string(),
  post_version: z.number(),
  scheduled_at: z.string(),
  scheduling_mode: z.enum(["queue", "exact"]).optional(),
  state: z.string(),
  payload: z.object({
    text: z.string(),
    media_ids: z.array(id),
    title: z.string(),
  }),
  external_id: z.string().nullable(),
  external_url: z.string().nullable(),
  last_error: z.string().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
});
export const ScheduleSchema = z.object({
  id,
  channel_id: id,
  days: z.array(z.number()),
  time: z.string(),
  timezone: z.string(),
  created_at: z.string(),
});
export const ProviderSchema = z
  .object({
    id: z.string().optional(),
    provider: z.string().optional(),
    name: z.string().optional(),
    display_name: z.string().optional(),
  })
  .passthrough();
export const HealthSchema = z.object({
  worker_last_seen: z.string().nullable(),
  worker_healthy: z.boolean(),
  queued: z.number(),
  uncertain: z.number(),
  manual_required: z.number(),
  oldest_due: z.string().nullable(),
});
export const AnalyticsSchema = z.object({
  availability: z.string(),
  reason: z.string().optional(),
  summary: z.object({
    scheduled: z.number(),
    published: z.number(),
    attention: z.number(),
  }),
  metrics: z.array(z.unknown()),
});
export const InboxSchema = z.object({
  availability: z.string(),
  reason: z.string().optional(),
  items: z.array(z.unknown()),
});
export type Brand = z.infer<typeof BrandSchema>;
export type Channel = z.infer<typeof ChannelSchema>;
export type Idea = z.infer<typeof IdeaSchema>;
export type Post = z.infer<typeof PostSchema>;
export type Media = z.infer<typeof MediaSchema>;
export type Publication = z.infer<typeof PublicationSchema>;
export type Schedule = z.infer<typeof ScheduleSchema>;
export type Provider = z.infer<typeof ProviderSchema>;
export type Health = z.infer<typeof HealthSchema>;
export class ApiError extends Error {
  constructor(
    message: string,
    public status: number,
    public code: string,
  ) {
    super(message);
  }
}
let sessionGeneration = 0;
const unauthorizedListeners = new Set<(generation: number) => void>();
export function advanceApiSession() {
  ++sessionGeneration;
}
export function onUnauthorized(listener: (generation: number) => void) {
  unauthorizedListeners.add(listener);
  return () => {
    unauthorizedListeners.delete(listener);
  };
}
export function isCurrentApiSession(generation: number) {
  return generation === sessionGeneration;
}
export async function authLock<T>(
  work: () => Promise<T>,
  signal?: AbortSignal,
) {
  if (!navigator.locks) throw new Error("Sign-in requires HTTPS or localhost.");
  return navigator.locks.request("beweft:authentication", { signal }, work);
}
export async function api<T>(
  path: string,
  schema: z.ZodType<T>,
  method = "GET",
  body?: unknown,
  signal?: AbortSignal,
): Promise<T> {
  const generation = sessionGeneration;
  const multipart = body instanceof FormData;
  const response = await fetch(`/api/v1${path}`, {
    method,
    credentials: "same-origin",
    signal,
    headers: {
      ...(method === "GET" ? {} : { "X-Beweft-Request": "1" }),
      ...(body && !multipart ? { "Content-Type": "application/json" } : {}),
    },
    body:
      body === undefined ? undefined : multipart ? body : JSON.stringify(body),
  });
  if (
    response.status === 401 &&
    !["/auth/login", "/auth/bootstrap", "/auth/status"].includes(path)
  )
    unauthorizedListeners.forEach((listener) => listener(generation));
  let result: unknown;
  try {
    result = await response.json();
  } catch {
    throw new ApiError(
      "The server returned an unreadable response. Please try again.",
      response.status,
      "invalid_response",
    );
  }
  if (!response.ok) {
    const error = z
      .object({ error: z.object({ code: z.string(), message: z.string() }) })
      .safeParse(result);
    throw new ApiError(
      error.success
        ? error.data.error.message
        : `Request failed (${response.status}).`,
      response.status,
      error.success ? error.data.error.code : "request_failed",
    );
  }
  const envelope = z.object({ data: schema }).safeParse(result);
  if (!envelope.success)
    throw new ApiError(
      "The server response did not match the API contract. Refresh after checking that the server and web app versions match.",
      response.status,
      "contract_mismatch",
    );
  return envelope.data.data;
}
export const ok = z.object({ ok: z.boolean() });
