import { useEffect, useRef, useState } from "react";
import { z } from "zod";
import {
  api,
  ApiError,
  captureWorkspaceSession,
  isCurrentWorkspaceSession,
  MediaSchema,
  requireWorkspaceSession,
  withWorkspaceSession,
  type ApiSession,
  type Media,
} from "../api";
import type { UpdateMediaRequest } from "../lib/api-contract.generated";

const Intent = z.object({
  id: z.string().uuid(),
  brand_id: z.string(),
  media_id: z.string(),
  metadata_version: z.number().int().positive(),
  alt_text: z.string(),
});
type Intent = z.infer<typeof Intent>;
type Phase = "checking" | "ready" | "pending" | "blocked" | "stale";
const read = (key: string) => {
  const raw = localStorage.getItem(key);
  return raw ? Intent.parse(JSON.parse(raw)) : null;
};

// The intent lock only protects storage transitions. No network request holds it
// or the authentication lock, so an explicit retry can fence a stalled write.
async function locked<T>(
  key: string,
  session: ApiSession,
  signal: AbortSignal,
  work: () => T,
) {
  if (!navigator.locks)
    throw new Error("Description recovery requires HTTPS or localhost.");
  return navigator.locks.request(key, { signal }, () =>
    withWorkspaceSession(
      session,
      () => {
        signal.throwIfAborted();
        return work();
      },
      signal,
    ),
  );
}

export function useMediaDescription(
  initial: Media,
  reconcile: (media: Media, session: ApiSession) => void,
) {
  const [media, setMedia] = useState(initial);
  const [text, setTextState] = useState(initial.alt_text);
  const [phase, setPhase] = useState<Phase>("checking");
  const [intent, setIntent] = useState<Intent | null>(null);
  const [status, setStatus] = useState("Checking the current description…");
  const [error, setError] = useState("");
  const [claiming, setClaiming] = useState(false);
  const alive = useRef(false);
  const key = useRef("");
  const request = useRef(0);
  const inspection = useRef<AbortController | null>(null);
  const claim = useRef<AbortController | null>(null);
  const mediaRef = useRef(initial);
  const textRef = useRef(initial.alt_text);
  const unsaved = useRef(false);
  const needsReview = useRef(false);
  const editSerial = useRef(0);
  const phaseRef = useRef<Phase>("checking");
  const claimingRef = useRef(false);
  const scope = `${initial.brand_id}:${initial.id}`;
  const replaceText = (value: string) => {
    unsaved.current = false;
    needsReview.current = false;
    textRef.current = value;
    setTextState(value);
  };
  const setText = (value: string) => {
    ++editSerial.current;
    unsaved.current = value !== mediaRef.current.alt_text;
    textRef.current = value;
    setTextState(value);
  };
  const phaseTo = (value: Phase) => {
    phaseRef.current = value;
    setPhase(value);
  };
  const active = (session: ApiSession) =>
    alive.current && isCurrentWorkspaceSession(session);

  const check = async (
    completed?: { intent: Intent; response: Media },
    resetUnsaved = false,
  ) => {
    let session: ApiSession;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    if (completed) {
      try {
        if (!key.current || read(key.current)?.id !== completed.intent.id)
          return;
      } catch {
        // The normal check will surface the storage error and block editing.
      }
    }
    const sequence = ++request.current;
    inspection.current?.abort();
    const controller = new AbortController();
    inspection.current = controller;
    const current = () =>
      active(session) &&
      sequence === request.current &&
      !controller.signal.aborted;
    phaseTo("checking");
    setError("");
    setStatus("Checking the current description…");
    try {
      if (!key.current) {
        const owner = await api(
          "/auth/me",
          z.object({ id: z.string() }),
          "GET",
          undefined,
          controller.signal,
        );
        if (!current()) return;
        key.current = `beweft.media-description:${owner.id}:${scope}`;
      }
      const fetched = await api(
        `/media/${initial.id}`,
        MediaSchema,
        "GET",
        undefined,
        controller.signal,
      );
      if (!current()) return;
      if (fetched.id !== initial.id || fetched.brand_id !== initial.brand_id)
        throw new Error("The server returned a different media asset.");
      // Reads can finish out of order with a more recent response already in
      // the workspace. Never roll that newer version back.
      const latest =
        mediaRef.current.metadata_version > fetched.metadata_version
          ? mediaRef.current
          : fetched;
      const result = await locked(
        key.current,
        session,
        controller.signal,
        () => {
          if (!current()) throw new DOMException("Abandoned", "AbortError");
          const remembered = read(key.current);
          if (
            remembered &&
            (remembered.brand_id !== initial.brand_id ||
              remembered.media_id !== initial.id)
          )
            throw new Error(
              "The saved description intent belongs to another asset.",
            );
          if (completed && remembered?.id !== completed.intent.id)
            return { ignored: true, remembered: null, resolved: false };
          if (
            remembered &&
            latest.metadata_version > remembered.metadata_version
          ) {
            localStorage.removeItem(key.current);
            if (read(key.current))
              throw new Error(
                "The resolved description intent could not be cleared from browser storage.",
              );
            return { ignored: false, remembered, resolved: true };
          }
          return { ignored: false, remembered, resolved: false };
        },
      );
      if (!current()) return;
      if (result.ignored) {
        void check();
        return;
      }
      setError("");
      const versionChanged =
        latest.metadata_version > mediaRef.current.metadata_version ||
        result.resolved;
      if (unsaved.current && versionChanged) needsReview.current = true;
      mediaRef.current = latest;
      setMedia(latest);
      reconcile(latest, session);
      if (result.remembered && !result.resolved) {
        setIntent(result.remembered);
        if (!unsaved.current || resetUnsaved)
          replaceText(result.remembered.alt_text);
        if (latest.metadata_version < result.remembered.metadata_version) {
          phaseTo("blocked");
          setError(
            "The server returned an older description version. Check again before editing.",
          );
          setStatus("");
        } else {
          phaseTo("pending");
          setStatus(
            unsaved.current
              ? "Another save may still be in progress. Your unsaved text is kept; check or retry the pending description."
              : "A previous save may still be in progress. Check again or retry this exact description.",
          );
        }
      } else {
        setIntent(null);
        if (
          result.resolved &&
          result.remembered &&
          !unsaved.current &&
          !resetUnsaved &&
          latest.alt_text !== result.remembered.alt_text
        ) {
          textRef.current = result.remembered.alt_text;
          setTextState(result.remembered.alt_text);
          unsaved.current = true;
          needsReview.current = true;
        }
        if (unsaved.current && !resetUnsaved && needsReview.current) {
          phaseTo("stale");
          setStatus(
            "The description changed elsewhere. Your unsaved text is kept; use the latest version before saving.",
          );
        } else if (unsaved.current && !resetUnsaved) {
          phaseTo("ready");
          setStatus("");
        } else {
          replaceText(latest.alt_text);
          phaseTo("ready");
        }
        if (!unsaved.current && result.resolved) {
          const confirmed =
            completed &&
            completed.response.metadata_version === latest.metadata_version &&
            completed.response.alt_text === latest.alt_text;
          setStatus(
            confirmed
              ? "Description saved."
              : "The description changed. Showing the current saved version.",
          );
        } else if (!unsaved.current) setStatus("");
      }
    } catch (failure) {
      if (!current()) return;
      phaseTo("blocked");
      setStatus("");
      setError(
        `${(failure as Error).message} Check the description before editing.`,
      );
    } finally {
      if (inspection.current === controller) inspection.current = null;
    }
  };

  useEffect(() => {
    alive.current = true;
    void check();
    const changed = (event: StorageEvent) => {
      if (!key.current || (event.key !== key.current && event.key !== null))
        return;
      // Disable stale controls synchronously, before the new storage read.
      void check();
    };
    window.addEventListener("storage", changed);
    return () => {
      alive.current = false;
      ++request.current;
      inspection.current?.abort();
      claim.current?.abort();
      window.removeEventListener("storage", changed);
    };
    // The parent mounts a fresh editor for each brand and media identity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scope]);

  useEffect(() => {
    if (
      !alive.current ||
      initial.metadata_version <= mediaRef.current.metadata_version
    )
      return;
    if (unsaved.current) {
      needsReview.current = true;
      mediaRef.current = initial;
      setMedia(initial);
      phaseTo("stale");
      setStatus(
        "The description changed elsewhere. Your unsaved text is kept; use the latest version before saving.",
      );
    } else void check();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initial.metadata_version]);

  const send = async (retry?: Intent) => {
    if (
      claimingRef.current ||
      !key.current ||
      (retry ? phaseRef.current !== "pending" : phaseRef.current !== "ready")
    )
      return;
    let session: ApiSession;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    const original = mediaRef.current;
    const requested = textRef.current;
    const editsAtClaim = editSerial.current;
    if (
      !retry &&
      (requested === original.alt_text || Array.from(requested).length > 2000)
    )
      return;
    claimingRef.current = true;
    setClaiming(true);
    setError("");
    const controller = new AbortController();
    claim.current = controller;
    try {
      const outgoing = await locked(
        key.current,
        session,
        controller.signal,
        () => {
          if (!active(session))
            throw new DOMException("Abandoned", "AbortError");
          const stored = read(key.current);
          if (
            stored &&
            (stored.brand_id !== initial.brand_id ||
              stored.media_id !== initial.id)
          )
            throw new Error(
              "The saved description intent belongs to another asset.",
            );
          if (retry)
            return stored?.id === retry.id &&
              stored.metadata_version === retry.metadata_version &&
              stored.alt_text === retry.alt_text
              ? { intent: stored, claimed: true }
              : null;
          if (stored) return { intent: stored, claimed: false };
          const next: Intent = {
            id: crypto.randomUUID(),
            brand_id: initial.brand_id,
            media_id: initial.id,
            metadata_version: original.metadata_version,
            alt_text: requested,
          };
          localStorage.setItem(key.current, JSON.stringify(next));
          if (read(key.current)?.id !== next.id)
            throw new Error(
              "The description intent could not be saved in browser storage.",
            );
          return { intent: next, claimed: true };
        },
      );
      if (!active(session)) return;
      if (!outgoing) {
        void check();
        return;
      }
      const { intent: savedIntent, claimed } = outgoing;
      setIntent(savedIntent);
      if (claimed && !retry && editSerial.current === editsAtClaim)
        replaceText(savedIntent.alt_text);
      phaseTo("pending");
      setStatus(
        unsaved.current
          ? "Another save is unconfirmed. Your unsaved text is kept; check or retry the pending description."
          : "This save is unconfirmed. Check again or retry this exact description.",
      );
      // If another tab claimed first, show its intent without sending it.
      if (!claimed) return;
      requireWorkspaceSession(session);
      const input = {
        alt_text: savedIntent.alt_text,
        metadata_version: savedIntent.metadata_version,
      } satisfies UpdateMediaRequest & { metadata_version: number };
      void api(`/media/${initial.id}`, MediaSchema, "PATCH", input)
        .then((response) => {
          if (!active(session)) return;
          if (
            response.id !== initial.id ||
            response.brand_id !== initial.brand_id ||
            response.metadata_version !== savedIntent.metadata_version + 1 ||
            response.alt_text !== savedIntent.alt_text
          )
            throw new Error(
              "The save response did not match the original description.",
            );
          void check({ intent: savedIntent, response });
        })
        .catch((failure: unknown) => {
          if (!active(session)) return;
          let stored: Intent | null;
          try {
            stored = read(key.current);
          } catch {
            stored = null;
          }
          if (stored?.id !== savedIntent.id) return;
          if (
            failure instanceof ApiError &&
            failure.code === "media_version_changed"
          ) {
            void check();
            return;
          }
          setError(
            `${(failure as Error).message} The save outcome is unconfirmed.`,
          );
        });
    } catch (failure) {
      if (active(session)) {
        phaseTo("blocked");
        setError(
          `${(failure as Error).message} Check the description before editing.`,
        );
      }
    } finally {
      if (claim.current === controller) claim.current = null;
      claimingRef.current = false;
      if (active(session)) setClaiming(false);
    }
  };

  return {
    media,
    text,
    setText,
    phase,
    intent,
    status,
    error,
    claiming,
    check,
    send,
  };
}
