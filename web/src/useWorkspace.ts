import { useCallback, useEffect, useRef, useState } from "react";
import { z } from "zod";
import {
  api,
  advanceApiSession,
  authLock,
  withOwnerCheck,
  ApiError,
  AuthStorageError,
  authEpochKey,
  BrandSchema,
  ChannelSchema,
  HealthSchema,
  IdeaSchema,
  MediaSchema,
  PostSchema,
  ProviderSchema,
  PublicationSchema,
  isCurrentApiSession,
  isCurrentWorkspaceSession,
  captureWorkspaceSession,
  requireWorkspaceSession,
  captureApiSession,
  onAuthIdentityChange,
  onAuthStorageError,
  onUnauthorized,
  readAuthEpoch,
  rotateAuthEpoch,
  setApiWorkspaceEpoch,
  ok,
  type Brand,
  type Channel,
  type Health,
  type Idea,
  type Media,
  type Post,
  type Provider,
  type Publication,
  type ApiSession,
} from "./api";
import { routeFromHash, type Route } from "./navigation";
type LoadArea = "brands" | "collections" | "providers" | "health";
const preference = (key: string, fallback: string) => {
  try {
    return localStorage.getItem(key) ?? fallback;
  } catch {
    return fallback;
  }
};
const savePreference = (key: string, value: string) => {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* Auth reports unavailable storage. */
  }
};

export function useWorkspace() {
  const [ready, setReadyState] = useState<boolean | null>(null);
  const [route, setRoute] = useState<Route>(routeFromHash);
  const [brands, setBrands] = useState<Brand[]>([]);
  const [brandId, setBrandId] = useState(preference("beweft.brand", ""));
  const [channels, setChannels] = useState<Channel[]>([]);
  const [posts, setPosts] = useState<Post[]>([]);
  const [ideas, setIdeas] = useState<Idea[]>([]);
  const [media, setMedia] = useState<Media[]>([]);
  const [publications, setPublications] = useState<Publication[]>([]);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [health, setHealth] = useState<Health | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [loadErrors, setLoadErrors] = useState<
    Partial<Record<LoadArea, string>>
  >({});
  const [toast, setToast] = useState("");
  const [pending, setPending] = useState(false);
  const [signingOut, setSigningOut] = useState(false);
  const [signOutError, setSignOutError] = useState("");
  const [sessionCheckFailed, setSessionCheckFailed] = useState(false);
  const signingOutRef = useRef(false);
  const [sidebar, setSidebar] = useState(false);
  const [composer, setComposer] = useState<
    (Partial<Post> & { composeDate?: string }) | null | undefined
  >(undefined);
  const [brandOpen, setBrandOpen] = useState(false);
  const [search, setSearch] = useState("");
  const [theme, setTheme] = useState(preference("beweft.theme", "system"));
  const readyRef = useRef<boolean | null>(null);
  const session = useRef(0);
  const sessionEpoch = useRef<string | null>(null);
  const verification = useRef(0);
  const verificationController = useRef<AbortController | null>(null);
  const [sessionRevision, setSessionRevision] = useState(0);
  const selection = useRef(0);
  // Only an explicit choice invalidates in-flight brand-list snapshots. An
  // automatic fallback still needs newer authoritative lists to be applied.
  const manualSelection = useRef(0);
  const requests = useRef(0);
  const applied = useRef<Record<LoadArea, number>>({
    brands: 0,
    collections: 0,
    providers: 0,
    health: 0,
  });
  const reported = useRef<Record<LoadArea, number>>({
    brands: 0,
    collections: 0,
    providers: 0,
    health: 0,
  });
  const finishedLoading = useRef(0);
  const actionGeneration = useRef(0);
  const metadataRequests = useRef<
    Partial<Record<"providers" | "health", AbortController>>
  >({});
  const context = useRef(brandId);
  const brand = brands.find((b) => b.id === brandId);
  const clearBrandData = useCallback(() => {
    setChannels([]);
    setPosts([]);
    setIdeas([]);
    setMedia([]);
    setPublications([]);
    setComposer(undefined);
  }, []);
  const abortMetadata = useCallback(() => {
    for (const controller of Object.values(metadataRequests.current))
      controller.abort();
    metadataRequests.current = {};
  }, []);
  useEffect(() => () => abortMetadata(), [abortMetadata]);
  const setReady = useCallback(
    (value: boolean, epoch?: string) => {
      if (value && !epoch)
        throw new Error("An authenticated workspace needs a session epoch.");
      if (
        readyRef.current === value &&
        (!value || sessionEpoch.current === epoch)
      )
        return;
      const clear =
        !value || (readyRef.current === true && sessionEpoch.current !== epoch);
      readyRef.current = value;
      sessionEpoch.current = value ? epoch! : null;
      setApiWorkspaceEpoch(sessionEpoch.current);
      ++session.current;
      advanceApiSession();
      setSessionRevision((current) => current + 1);
      if (value) {
        setSignOutError("");
        setSessionCheckFailed(false);
      }
      if (clear) {
        abortMetadata();
        verificationController.current?.abort();
        ++selection.current;
        ++actionGeneration.current;
        clearBrandData();
        setBrands([]);
        setProviders([]);
        setHealth(null);
        setPending(false);
        setBrandOpen(false);
        setSidebar(false);
        setSearch("");
        setToast("");
        setError("");
        setLoadErrors({});
        setLoading(true);
      }
      setReadyState(value);
    },
    [abortMetadata, clearBrandData],
  );
  const verifySession = useCallback(async () => {
    if (signingOutRef.current) return;
    setSessionCheckFailed(false);
    setSignOutError("");
    verificationController.current?.abort();
    const controller = new AbortController();
    verificationController.current = controller;
    const check = ++verification.current;
    let captured: ApiSession | undefined;
    try {
      await withOwnerCheck(async (ownerSignal) => {
        if (check !== verification.current || signingOutRef.current) return;
        captured = captureApiSession();
        await api(
          "/auth/me",
          z.object({ id: z.string(), email: z.string() }),
          "GET",
          undefined,
          ownerSignal,
          true,
        );
        if (
          check === verification.current &&
          isCurrentApiSession(captured) &&
          !signingOutRef.current
        )
          setReady(true, captured.epoch);
      }, controller.signal);
    } catch (failure) {
      if (
        controller.signal.aborted ||
        check !== verification.current ||
        (captured && !isCurrentApiSession(captured))
      )
        return;
      setReady(false);
      if (failure instanceof AuthStorageError) {
        setSignOutError(failure.message);
        setSessionCheckFailed(true);
      } else if (!(failure instanceof ApiError && failure.status === 401)) {
        setSignOutError(
          failure instanceof Error
            ? failure.message
            : "Unable to check your session.",
        );
        setSessionCheckFailed(true);
      }
    } finally {
      if (verificationController.current === controller)
        verificationController.current = null;
    }
  }, [setReady]);
  const identityChanged = useCallback(() => {
    let epoch: string;
    try {
      epoch = readAuthEpoch();
    } catch {
      return;
    }
    if (readyRef.current === true && sessionEpoch.current === epoch) return;
    if (readyRef.current === true) setReady(false);
    if (!signingOutRef.current) void verifySession();
  }, [setReady, verifySession]);
  const acceptSession = useCallback(
    (epoch: string) => {
      try {
        if (readAuthEpoch() === epoch) setReady(true, epoch);
        else {
          setReady(false);
          void verifySession();
        }
      } catch {
        /* The storage listener presents the error. */
      }
    },
    [setReady, verifySession],
  );
  useEffect(
    () =>
      onUnauthorized((requestSession) => {
        if (
          readyRef.current === true &&
          sessionEpoch.current === requestSession.epoch &&
          isCurrentApiSession(requestSession)
        )
          setReady(false);
      }),
    [setReady],
  );
  useEffect(() => {
    const removeIdentity = onAuthIdentityChange(identityChanged);
    const removeStorageError = onAuthStorageError((failure) => {
      setReady(false);
      setSignOutError(failure.message);
    });
    const storage = (event: StorageEvent) => {
      if (event.key === authEpochKey || event.key === null) identityChanged();
    };
    window.addEventListener("storage", storage);
    return () => {
      removeIdentity();
      removeStorageError();
      window.removeEventListener("storage", storage);
    };
  }, [identityChanged, setReady]);
  useEffect(() => {
    void verifySession();
    return () => {
      ++verification.current;
      verificationController.current?.abort();
    };
  }, [verifySession]);
  const signOut = async () => {
    if (signingOutRef.current || readyRef.current !== true) return;
    const observedEpoch = sessionEpoch.current;
    signingOutRef.current = true;
    setSigningOut(true);
    setSignOutError("");
    setReady(false);
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 15000);
    let superseded = false;
    try {
      await authLock(async () => {
        if (!observedEpoch || readAuthEpoch() !== observedEpoch) {
          superseded = true;
          return;
        }
        rotateAuthEpoch();
        await api("/auth/logout", ok, "POST", {}, controller.signal);
      }, controller.signal);
    } catch (failure) {
      if (failure instanceof AuthStorageError) setSignOutError(failure.message);
      else if (!(failure instanceof ApiError && failure.status === 401))
        setSignOutError(
          "The server could not confirm sign-out. Sign in again and retry when it is reachable.",
        );
    } finally {
      clearTimeout(timeout);
      signingOutRef.current = false;
      setSigningOut(false);
      if (superseded) void verifySession();
    }
  };
  // Credential-changing operations share the sign-in lock and only clear the
  // session that initiated them. A lost response is treated as unconfirmed.
  const endSessionWith = async (
    path: string,
    method: "POST" | "DELETE",
    body?: unknown,
  ) => {
    const observed = captureWorkspaceSession();
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 15000);
    try {
      await authLock(async () => {
        requireWorkspaceSession(observed);
        try {
          await api(path, ok, method, body, controller.signal);
        } catch (failure) {
          if (
            path === "/auth/password" &&
            failure instanceof ApiError &&
            failure.status === 401 &&
            isCurrentWorkspaceSession(observed)
          ) {
            await api(
              "/auth/me",
              z.object({ id: z.string(), email: z.string() }),
              "GET",
              undefined,
              controller.signal,
              true,
            );
            throw new Error("The current password was not accepted.");
          }
          const ambiguous =
            !(failure instanceof ApiError) ||
            failure.status >= 500 ||
            failure.code === "invalid_response" ||
            failure.code === "contract_mismatch";
          if (ambiguous && isCurrentWorkspaceSession(observed)) {
            rotateAuthEpoch();
            setReady(false);
            setSignOutError(
              "The security change could not be confirmed. Sign in again and check your sessions.",
            );
          }
          throw failure;
        }
        if (!isCurrentWorkspaceSession(observed)) return;
        rotateAuthEpoch();
        setReady(false);
      }, controller.signal);
    } finally {
      clearTimeout(timeout);
    }
  };
  useEffect(() => {
    const listener = () => setRoute(routeFromHash());
    window.addEventListener("hashchange", listener);
    return () => window.removeEventListener("hashchange", listener);
  }, []);
  useEffect(() => {
    const match = matchMedia("(prefers-color-scheme: dark)");
    const apply = () =>
      (document.documentElement.dataset.theme =
        theme === "system" ? (match.matches ? "dark" : "light") : theme);
    apply();
    savePreference("beweft.theme", theme);
    match.addEventListener("change", apply);
    return () => match.removeEventListener("change", apply);
  }, [theme]);
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(""), 5000);
    return () => clearTimeout(t);
  }, [toast]);
  const selectBrand = useCallback(
    (id: string) => {
      abortMetadata();
      ++selection.current;
      context.current = id;
      setError("");
      setLoadErrors({});
      setToast("");
      clearBrandData();
      setBrandId(id);
      savePreference("beweft.brand", id);
      setSidebar(false);
      setSearch("");
      setLoading(true);
    },
    [abortMetadata, clearBrandData],
  );
  const chooseBrand = useCallback(
    (id: string) => {
      ++manualSelection.current;
      selectBrand(id);
    },
    [selectBrand],
  );
  const load = useCallback(async () => {
    if (!ready || context.current !== brandId) return;
    let requestSession: ApiSession;
    try {
      requestSession = captureApiSession();
    } catch {
      return;
    }
    if (requestSession.epoch !== sessionEpoch.current) {
      identityChanged();
      return;
    }
    const requestId = ++requests.current;
    const sessionId = session.current;
    const selectionId = selection.current;
    const manualSelectionId = manualSelection.current;
    const currentSession = () =>
      readyRef.current === true &&
      session.current === sessionId &&
      sessionEpoch.current === requestSession.epoch &&
      isCurrentApiSession(requestSession);
    const currentSelection = () =>
      currentSession() &&
      selection.current === selectionId &&
      context.current === brandId;
    const currentBrandList = () =>
      currentSession() && manualSelection.current === manualSelectionId;
    const report = (area: LoadArea, message: string) => {
      if (
        !(area === "brands" ? currentBrandList() : currentSelection()) ||
        requestId < reported.current[area]
      )
        return;
      reported.current[area] = requestId;
      setLoadErrors((previous) => ({ ...previous, [area]: message }));
    };
    const apply = (area: LoadArea, update: () => void) => {
      if (
        !(area === "brands" ? currentBrandList() : currentSelection()) ||
        requestId < applied.current[area]
      )
        return false;
      applied.current[area] = requestId;
      update();
      return true;
    };
    const metadata = <T>(
      area: "providers" | "health",
      request: (signal: AbortSignal) => Promise<T>,
      update: (value: T) => void,
    ) => {
      metadataRequests.current[area]?.abort();
      const controller = new AbortController();
      metadataRequests.current[area] = controller;
      void request(controller.signal)
        .then(
          (value) => {
            if (controller.signal.aborted) return;
            apply(area, () => update(value));
            report(area, "");
          },
          (e: unknown) => {
            if (!controller.signal.aborted) report(area, (e as Error).message);
          },
        )
        .finally(() => {
          if (metadataRequests.current[area] === controller)
            delete metadataRequests.current[area];
        });
    };
    setLoading(true);
    metadata(
      "providers",
      (signal) =>
        api("/providers", z.array(ProviderSchema), "GET", undefined, signal),
      setProviders,
    );
    metadata(
      "health",
      (signal) => api("/jobs/health", HealthSchema, "GET", undefined, signal),
      setHealth,
    );
    let area: LoadArea = "brands";
    try {
      const bs = await api("/brands", z.array(BrandSchema));
      if (!apply("brands", () => setBrands(bs))) return;
      report("brands", "");
      const selected = context.current;
      const next = bs.some((b) => b.id === selected)
        ? selected
        : (bs[0]?.id ?? "");
      if (next !== selected) {
        selectBrand(next);
        return;
      }
      if (!currentSelection()) return;
      area = "collections";
      if (next) {
        const q = `?brand_id=${encodeURIComponent(next)}`;
        const [cs, posts, ins, ms, pubs] = await Promise.all([
          api(`/channels${q}`, z.array(ChannelSchema)),
          api(`/posts${q}`, z.array(PostSchema)),
          api(`/ideas${q}`, z.array(IdeaSchema)),
          api(`/media${q}`, z.array(MediaSchema)),
          api(`/publications${q}`, z.array(PublicationSchema)),
        ]);
        apply("collections", () => {
          setChannels(cs);
          setPosts(posts);
          setIdeas(ins);
          setMedia((previous) =>
            ms.map((item) => {
              const known = previous.find((row) => row.id === item.id);
              return known && known.metadata_version > item.metadata_version
                ? known
                : item;
            }),
          );
          setPublications(pubs);
        });
      } else {
        apply("collections", clearBrandData);
      }
      report("collections", "");
    } catch (e: unknown) {
      report(area, (e as Error).message);
    } finally {
      if (currentSelection() && requestId >= finishedLoading.current) {
        finishedLoading.current = requestId;
        setLoading(false);
      }
    }
  }, [
    ready,
    brandId,
    selectBrand,
    clearBrandData,
    identityChanged,
    sessionRevision,
  ]);
  useEffect(() => {
    void load();
  }, [load]);
  useEffect(() => {
    if (!ready) return;
    const timer = setInterval(() => void load(), 30000);
    return () => clearInterval(timer);
  }, [ready, load]);
  const action = async (
    fn: () => Promise<unknown>,
    message = "Saved",
    guard: "selection" | "session" = "selection",
    onCommitted?: () => void,
  ) => {
    if (pending) return false;
    let requestSession: ApiSession;
    try {
      requestSession = captureApiSession();
    } catch {
      return false;
    }
    if (requestSession.epoch !== sessionEpoch.current) {
      identityChanged();
      return false;
    }
    const actionId = ++actionGeneration.current;
    const sessionId = session.current;
    const selectionId = selection.current;
    const currentAction = () =>
      readyRef.current === true &&
      session.current === sessionId &&
      sessionEpoch.current === requestSession.epoch &&
      isCurrentApiSession(requestSession) &&
      (guard === "session" || selection.current === selectionId);
    setPending(true);
    setError("");
    try {
      await fn();
      if (!currentAction()) return false;
      onCommitted?.();
      await load();
      if (!currentAction()) return false;
      setToast(message);
      return true;
    } catch (e: unknown) {
      if (currentAction() && !(e instanceof ApiError && e.status === 401))
        setError((e as Error).message);
      return false;
    } finally {
      if (actionGeneration.current === actionId) setPending(false);
    }
  };
  const createBrand = async (name: string) => {
    const manualSelectionId = manualSelection.current;
    let created: Brand | undefined;
    await action(
      async () => {
        created = await api("/brands", BrandSchema, "POST", { name });
      },
      "Brand created",
      "session",
      () => {
        setBrandOpen(false);
        if (created && manualSelection.current === manualSelectionId)
          chooseBrand(created.id);
      },
    );
  };
  const go = (r: Route) => {
    location.hash = r;
    setRoute(r);
    setSidebar(false);
    setSearch("");
  };
  const reconcileMedia = useCallback(
    (updated: Media, requestSession: ApiSession) => {
      if (
        !readyRef.current ||
        context.current !== updated.brand_id ||
        sessionEpoch.current !== requestSession.epoch ||
        !isCurrentApiSession(requestSession)
      )
        return;
      setMedia((previous) =>
        previous.map((item) =>
          item.id === updated.id &&
          updated.metadata_version >= item.metadata_version
            ? updated
            : item,
        ),
      );
    },
    [],
  );
  return {
    ready,
    acceptSession,
    signingOut,
    signOutError,
    sessionCheckFailed,
    retrySession: verifySession,
    signOut,
    endSessionWith,
    route,
    brands,
    brandId,
    channels,
    posts,
    ideas,
    media,
    publications,
    providers,
    health,
    loading,
    error:
      error ||
      loadErrors.brands ||
      loadErrors.collections ||
      loadErrors.providers ||
      loadErrors.health ||
      "",
    toast,
    pending,
    sidebar,
    setSidebar,
    composer,
    setComposer,
    brandOpen,
    setBrandOpen,
    search,
    setSearch,
    theme,
    setTheme,
    brand,
    load,
    action,
    reconcileMedia,
    go,
    chooseBrand,
    createBrand,
  };
}
