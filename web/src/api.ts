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
  alt_text: z.string(),
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
export const authEpochKey = "beweft.auth-epoch";
export type ApiSession = { generation: number; epoch: string };
export class AuthStorageError extends Error {
  constructor() {
    super(
      "Browser storage is unavailable. Enable site storage for beweft, then reload before signing in.",
    );
  }
}
let workspaceEpoch: string | null = null;
const unauthorizedListeners = new Set<(session: ApiSession) => void>();
const identityListeners = new Set<() => void>();
const storageListeners = new Set<(error: AuthStorageError) => void>();
export function readAuthEpoch() {
  try {
    let epoch = localStorage.getItem(authEpochKey);
    if (!epoch) {
      epoch = crypto.randomUUID();
      localStorage.setItem(authEpochKey, epoch);
      epoch = localStorage.getItem(authEpochKey);
    }
    if (!epoch) throw new Error("Auth epoch was not saved");
    return epoch;
  } catch {
    const failure = new AuthStorageError();
    storageListeners.forEach((listener) => listener(failure));
    throw failure;
  }
}
export function rotateAuthEpoch() {
  try {
    const epoch = crypto.randomUUID();
    localStorage.setItem(authEpochKey, epoch);
    if (localStorage.getItem(authEpochKey) !== epoch)
      throw new Error("Auth epoch was not saved");
    return epoch;
  } catch {
    const failure = new AuthStorageError();
    storageListeners.forEach((listener) => listener(failure));
    throw failure;
  }
}
export function captureApiSession(): ApiSession {
  return { generation: sessionGeneration, epoch: readAuthEpoch() };
}
export function setApiWorkspaceEpoch(epoch: string | null) {
  workspaceEpoch = epoch;
}
export function advanceApiSession() {
  ++sessionGeneration;
}
export function onUnauthorized(listener: (session: ApiSession) => void) {
  unauthorizedListeners.add(listener);
  return () => {
    unauthorizedListeners.delete(listener);
  };
}
export function onAuthIdentityChange(listener: () => void) {
  identityListeners.add(listener);
  return () => {
    identityListeners.delete(listener);
  };
}
export function onAuthStorageError(
  listener: (error: AuthStorageError) => void,
) {
  storageListeners.add(listener);
  return () => {
    storageListeners.delete(listener);
  };
}
export function isCurrentApiSession(session: ApiSession) {
  try {
    return (
      session.generation === sessionGeneration &&
      session.epoch === readAuthEpoch()
    );
  } catch {
    return false;
  }
}
export function isCurrentWorkspaceSession(session: ApiSession) {
  return workspaceEpoch === session.epoch && isCurrentApiSession(session);
}
export function requireWorkspaceSession(session: ApiSession) {
  if (isCurrentWorkspaceSession(session)) return;
  if (workspaceEpoch !== null)
    identityListeners.forEach((listener) => listener());
  throw new ApiError(
    "The browser session changed. Refreshing your workspace.",
    401,
    "session_changed",
  );
}
export function captureWorkspaceSession() {
  const session = captureApiSession();
  requireWorkspaceSession(session);
  return session;
}
function requireCurrentApiSession(session: ApiSession) {
  if (isCurrentApiSession(session)) return;
  if (workspaceEpoch !== null)
    identityListeners.forEach((listener) => listener());
  throw new ApiError(
    "The browser session changed. Refreshing your workspace.",
    401,
    "session_changed",
  );
}
export async function authLock<T>(
  work: () => Promise<T>,
  signal?: AbortSignal,
) {
  if (!navigator.locks) throw new Error("Sign-in requires HTTPS or localhost.");
  return navigator.locks.request("beweft:authentication", { signal }, work);
}
// Owner reads share the credential lock, but must not delay sign-in or sign-out
// for an unbounded queue or a response whose body never arrives.
export async function withOwnerCheck<T>(
  work: (signal: AbortSignal) => Promise<T>,
  signal?: AbortSignal,
) {
  const controller = new AbortController();
  const cancel = () => controller.abort();
  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    controller.abort();
  }, 5000);
  if (signal?.aborted) cancel();
  else signal?.addEventListener("abort", cancel, { once: true });
  try {
    return await authLock(() => work(controller.signal), controller.signal);
  } catch (failure) {
    if (timedOut)
      throw new ApiError(
        "The session check timed out. Please try again.",
        0,
        "timeout",
      );
    throw failure;
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener("abort", cancel);
  }
}
// Keep local intent changes atomic with respect to app-controlled credential
// transitions. The callback must finish synchronously; never hold this lock
// across a network request.
export function withWorkspaceSession<T>(
  session: ApiSession,
  work: () => T,
  signal?: AbortSignal,
) {
  return withOwnerCheck(async (ownerSignal) => {
    ownerSignal.throwIfAborted();
    requireWorkspaceSession(session);
    return work();
  }, signal);
}
export async function api<T>(
  path: string,
  schema: z.ZodType<T>,
  method = "GET",
  body?: unknown,
  signal?: AbortSignal,
  authLockHeld = false,
): Promise<T> {
  // Direct callers that inspect the owner must not observe an intermediate
  // credential transition. Workspace verification already holds this lock.
  if (path === "/auth/me" && !authLockHeld) {
    return withOwnerCheck((ownerSignal) => {
      const epoch = readAuthEpoch();
      if (workspaceEpoch !== epoch) {
        if (workspaceEpoch !== null)
          identityListeners.forEach((listener) => listener());
        throw new ApiError(
          "The browser session changed. Refreshing your workspace.",
          401,
          "session_changed",
        );
      }
      return api(path, schema, method, body, ownerSignal, true);
    }, signal);
  }
  const session = captureApiSession();
  if (
    ![
      "/auth/me",
      "/auth/status",
      "/auth/login",
      "/auth/bootstrap",
      "/auth/logout",
    ].includes(path)
  ) {
    if (workspaceEpoch !== session.epoch) {
      if (workspaceEpoch !== null)
        identityListeners.forEach((listener) => listener());
      throw new ApiError(
        "The browser session changed. Refreshing your workspace.",
        401,
        "session_changed",
      );
    }
  }
  const multipart = body instanceof FormData;
  // GET deadlines cover the whole response, including a body that stalls
  // after headers arrive. Mutation outcomes are never retried or timed out here.
  const controller = method === "GET" ? new AbortController() : null;
  const cancel = () => controller?.abort();
  let timedOut = false;
  const timer = controller
    ? setTimeout(
        () => {
          timedOut = true;
          controller.abort();
        },
        ["/providers", "/jobs/health"].includes(path)
          ? 5000
          : path === "/brands"
            ? 90000
            : 15000,
      )
    : null;
  if (signal?.aborted) cancel();
  else if (controller && signal)
    signal.addEventListener("abort", cancel, { once: true });
  const handleFailure = (failure: unknown): never => {
    requireCurrentApiSession(session);
    if (timedOut)
      throw new ApiError(
        "The request timed out. Please try again.",
        0,
        "timeout",
      );
    throw failure;
  };
  try {
    let response: Response;
    try {
      response = await fetch(`/api/v1${path}`, {
        method,
        credentials: "same-origin",
        signal: controller?.signal ?? signal,
        headers: {
          ...(method === "GET" ? {} : { "X-Beweft-Request": "1" }),
          ...(body && !multipart ? { "Content-Type": "application/json" } : {}),
        },
        body:
          body === undefined
            ? undefined
            : multipart
              ? body
              : JSON.stringify(body),
      });
    } catch (failure) {
      return handleFailure(failure);
    }
    requireCurrentApiSession(session);
    if (
      response.status === 401 &&
      !["/auth/login", "/auth/bootstrap", "/auth/status"].includes(path)
    )
      unauthorizedListeners.forEach((listener) => listener(session));
    let result: unknown;
    try {
      result = await response.json();
    } catch (failure) {
      if (timedOut || signal?.aborted) handleFailure(failure);
      requireCurrentApiSession(session);
      throw new ApiError(
        "The server returned an unreadable response. Please try again.",
        response.status,
        "invalid_response",
      );
    }
    requireCurrentApiSession(session);
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
  } finally {
    if (timer !== null) clearTimeout(timer);
    if (controller && signal) signal.removeEventListener("abort", cancel);
  }
}
export const ok = z.object({ ok: z.boolean() });
