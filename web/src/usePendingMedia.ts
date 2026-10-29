import { useEffect, useRef, useState } from "react";
import { z } from "zod";
import {
  api,
  captureWorkspaceSession,
  isCurrentWorkspaceSession,
  MediaSchema,
  requireWorkspaceSession,
  type ApiSession,
  type Media,
} from "./api";
import {
  mediaIntentEvent,
  mediaIntentPrefix,
  readMediaIntent,
  type MediaIntent,
} from "./mediaIntent";

export type PendingMedia = {
  owner: string;
  brand: string;
  session: ApiSession | null;
  intents: Record<string, MediaIntent>;
  checking: string[];
  assetErrors: Record<string, string>;
  ready: boolean;
  error: string;
  retry: () => void;
};

// Keep recovery visible even when the media dialog is closed. A removed intent
// is followed by a fresh per-asset read before scheduling can become available.
export function usePendingMedia(
  brand: string,
  reconcile: (media: Media, session: ApiSession) => void,
): PendingMedia {
  const [state, setState] = useState<Omit<PendingMedia, "retry">>({
    owner: "",
    brand: "",
    session: null,
    intents: {},
    checking: [],
    assetErrors: {},
    ready: false,
    error: "",
  });
  const [attempt, setAttempt] = useState(0);
  const recovery = useRef({
    brand: "",
    epoch: "",
    refreshIds: new Set<string>(),
    lastIds: new Set<string>(),
  });
  useEffect(() => {
    if (!brand) return;
    let active = true,
      owner = "",
      prefix = "",
      sequence = 0;
    let controller: AbortController | null = null;
    let session: ApiSession;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    if (
      recovery.current.brand !== brand ||
      recovery.current.epoch !== session.epoch
    )
      recovery.current = {
        brand,
        epoch: session.epoch,
        refreshIds: new Set(),
        lastIds: new Set(),
      };
    const refreshIds = recovery.current.refreshIds;
    const current = () => active && isCurrentWorkspaceSession(session);
    setState({
      owner: "",
      brand,
      session: null,
      intents: {},
      checking: [],
      assetErrors: {},
      ready: false,
      error: "",
    });
    const scan = async (changed?: string | null) => {
      if (!current() || !prefix) return;
      if (changed === null) {
        recovery.current.lastIds.forEach((id) => refreshIds.add(id));
      } else if (changed?.startsWith(prefix))
        refreshIds.add(changed.slice(prefix.length));
      const id = ++sequence;
      controller?.abort();
      controller = new AbortController();
      const signal = controller.signal;
      setState((previous) => ({ ...previous, ready: false, error: "" }));
      try {
        requireWorkspaceSession(session);
        const intents: Record<string, MediaIntent> = {};
        for (let i = 0; i < localStorage.length; i++) {
          const key = localStorage.key(i);
          if (!key?.startsWith(prefix)) continue;
          const intent = readMediaIntent(key);
          const mediaId = key.slice(prefix.length);
          if (
            !intent ||
            intent.brand_id !== brand ||
            intent.media_id !== mediaId
          )
            throw new Error(
              "A pending media description could not be read safely.",
            );
          intents[mediaId] = intent;
        }
        recovery.current.lastIds.forEach((mediaId) => {
          if (!intents[mediaId]) refreshIds.add(mediaId);
        });
        recovery.current.lastIds = new Set(Object.keys(intents));
        const settled = [...refreshIds].filter((mediaId) => !intents[mediaId]);
        const toRefresh = [...new Set([...settled, ...Object.keys(intents)])];
        setState({
          owner,
          brand,
          session,
          intents,
          checking: toRefresh,
          assetErrors: {},
          ready: true,
          error: "",
        });
        await Promise.all(
          toRefresh.map(async (mediaId) => {
            let refreshed: Media | null = null;
            try {
              const row = await api(
                `/media/${mediaId}`,
                MediaSchema,
                "GET",
                undefined,
                signal,
              );
              if (row.id === mediaId && row.brand_id === brand) refreshed = row;
            } catch {
              /* Keep only this asset blocked and offer a retry. */
            }
            if (!current() || signal.aborted || id !== sequence) return;
            if (refreshed) {
              reconcile(refreshed, session);
              if (!intents[mediaId]) refreshIds.delete(mediaId);
            }
            setState((previous) => {
              if (id !== sequence) return previous;
              const assetErrors = { ...previous.assetErrors };
              if (refreshed) delete assetErrors[mediaId];
              else
                assetErrors[mediaId] =
                  "This media description could not be refreshed.";
              return {
                ...previous,
                checking: previous.checking.filter(
                  (value) => value !== mediaId,
                ),
                assetErrors,
              };
            });
          }),
        );
      } catch (failure) {
        if (!current() || signal.aborted || id !== sequence) return;
        setState((previous) => ({
          ...previous,
          owner,
          brand,
          session,
          ready: false,
          error: `${(failure as Error).message} Media descriptions must be checked before scheduling.`,
        }));
      }
    };
    const storage = (event: StorageEvent) => {
      if (event.key === null || event.key.startsWith(prefix))
        void scan(event.key);
    };
    const local = (event: Event) => {
      const key = (event as CustomEvent<string>).detail;
      if (key?.startsWith(prefix)) void scan(key);
    };
    window.addEventListener("storage", storage);
    window.addEventListener(mediaIntentEvent, local);
    const ownerCheck = new AbortController();
    void api(
      "/auth/me",
      z.object({ id: z.string() }),
      "GET",
      undefined,
      ownerCheck.signal,
    )
      .then((identity) => {
        if (!current()) return;
        owner = identity.id;
        prefix = mediaIntentPrefix(owner, brand);
        void scan();
      })
      .catch((failure: unknown) => {
        if (current() && !ownerCheck.signal.aborted)
          setState({
            owner: "",
            brand,
            session: null,
            intents: {},
            checking: [],
            assetErrors: {},
            ready: false,
            error: `${(failure as Error).message} Media descriptions must be checked before scheduling.`,
          });
      });
    return () => {
      active = false;
      ++sequence;
      ownerCheck.abort();
      controller?.abort();
      window.removeEventListener("storage", storage);
      window.removeEventListener(mediaIntentEvent, local);
    };
  }, [brand, reconcile, attempt]);
  const currentBrand = state.brand === brand && !!brand;
  return {
    ...state,
    ready: currentBrand && state.ready,
    retry: () => setAttempt((value) => value + 1),
  };
}
