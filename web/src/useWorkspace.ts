import { useCallback, useEffect, useRef, useState } from "react";
import { z } from "zod";
import {
  api,
  ApiError,
  BrandSchema,
  ChannelSchema,
  HealthSchema,
  IdeaSchema,
  MediaSchema,
  PostSchema,
  ProviderSchema,
  PublicationSchema,
  type Brand,
  type Channel,
  type Health,
  type Idea,
  type Media,
  type Post,
  type Provider,
  type Publication,
} from "./api";
import { routeFromHash, type Route } from "./navigation";
type LoadArea = "brands" | "collections" | "providers" | "health";

export function useWorkspace() {
  const [ready, setReadyState] = useState<boolean | null>(null);
  const [route, setRoute] = useState<Route>(routeFromHash);
  const [brands, setBrands] = useState<Brand[]>([]);
  const [brandId, setBrandId] = useState(
    localStorage.getItem("beweft.brand") ?? "",
  );
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
  const [sidebar, setSidebar] = useState(false);
  const [composer, setComposer] = useState<
    (Partial<Post> & { composeDate?: string }) | null | undefined
  >(undefined);
  const [brandOpen, setBrandOpen] = useState(false);
  const [search, setSearch] = useState("");
  const [theme, setTheme] = useState(
    localStorage.getItem("beweft.theme") ?? "system",
  );
  const readyRef = useRef<boolean | null>(null);
  const session = useRef(0);
  const selection = useRef(0);
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
  const setReady = useCallback(
    (value: boolean) => {
      if (readyRef.current === value) return;
      readyRef.current = value;
      ++session.current;
      if (!value) {
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
    [clearBrandData],
  );
  useEffect(() => {
    api("/auth/me", z.object({ id: z.string(), email: z.string() }))
      .then(() => setReady(true))
      .catch((e) => {
        if (e instanceof ApiError && e.status === 401) setReady(false);
        else {
          setReady(false);
          setError(e.message);
        }
      });
  }, [setReady]);
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
    localStorage.setItem("beweft.theme", theme);
    match.addEventListener("change", apply);
    return () => match.removeEventListener("change", apply);
  }, [theme]);
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(""), 5000);
    return () => clearTimeout(t);
  }, [toast]);
  const chooseBrand = useCallback(
    (id: string) => {
      ++selection.current;
      context.current = id;
      setError("");
      setLoadErrors({});
      setToast("");
      clearBrandData();
      setBrandId(id);
      localStorage.setItem("beweft.brand", id);
      setSidebar(false);
      setSearch("");
      setLoading(true);
    },
    [clearBrandData],
  );
  const load = useCallback(async () => {
    if (!ready || context.current !== brandId) return;
    const requestId = ++requests.current;
    const sessionId = session.current;
    const selectionId = selection.current;
    const currentSession = () =>
      readyRef.current === true && session.current === sessionId;
    const currentSelection = () =>
      currentSession() &&
      selection.current === selectionId &&
      context.current === brandId;
    const watchAuth = <T>(promise: Promise<T>) =>
      promise.catch((e: unknown) => {
        if (e instanceof ApiError && e.status === 401 && currentSession())
          setReady(false);
        throw e;
      });
    const report = (area: LoadArea, message: string) => {
      if (!currentSelection() || requestId < reported.current[area]) return;
      reported.current[area] = requestId;
      setLoadErrors((previous) => ({ ...previous, [area]: message }));
    };
    const apply = (area: LoadArea, update: () => void) => {
      if (!currentSelection() || requestId < applied.current[area])
        return false;
      applied.current[area] = requestId;
      update();
      return true;
    };
    const metadata = <T>(
      area: "providers" | "health",
      promise: Promise<T>,
      update: (value: T) => void,
    ) => {
      void watchAuth(promise).then(
        (value) => {
          apply(area, () => update(value));
          report(area, "");
        },
        (e: unknown) => report(area, (e as Error).message),
      );
    };
    setLoading(true);
    metadata(
      "providers",
      api("/providers", z.array(ProviderSchema)),
      setProviders,
    );
    metadata("health", api("/jobs/health", HealthSchema), setHealth);
    let area: LoadArea = "brands";
    try {
      const bs = await watchAuth(api("/brands", z.array(BrandSchema)));
      if (!apply("brands", () => setBrands(bs))) return;
      report("brands", "");
      const next = bs.some((b) => b.id === brandId)
        ? brandId
        : (bs[0]?.id ?? "");
      if (next !== brandId) {
        chooseBrand(next);
        return;
      }
      area = "collections";
      if (next) {
        const q = `?brand_id=${encodeURIComponent(next)}`;
        const [cs, posts, ins, ms, pubs] = await Promise.all([
          watchAuth(api(`/channels${q}`, z.array(ChannelSchema))),
          watchAuth(api(`/posts${q}`, z.array(PostSchema))),
          watchAuth(api(`/ideas${q}`, z.array(IdeaSchema))),
          watchAuth(api(`/media${q}`, z.array(MediaSchema))),
          watchAuth(api(`/publications${q}`, z.array(PublicationSchema))),
        ]);
        apply("collections", () => {
          setChannels(cs);
          setPosts(posts);
          setIdeas(ins);
          setMedia(ms);
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
  }, [ready, brandId, chooseBrand, clearBrandData, setReady]);
  useEffect(() => {
    void load();
  }, [load]);
  useEffect(() => {
    if (!ready) return;
    const timer = setInterval(() => void load(), 30000);
    return () => clearInterval(timer);
  }, [ready, load]);
  const action = async (fn: () => Promise<unknown>, message = "Saved") => {
    if (pending) return false;
    const actionId = ++actionGeneration.current;
    const sessionId = session.current;
    const selectionId = selection.current;
    const currentAction = () =>
      readyRef.current === true &&
      session.current === sessionId &&
      selection.current === selectionId;
    setPending(true);
    setError("");
    try {
      await fn();
      if (!currentAction()) return false;
      await load();
      if (!currentAction()) return false;
      setToast(message);
      return true;
    } catch (e: unknown) {
      if (
        e instanceof ApiError &&
        e.status === 401 &&
        readyRef.current &&
        session.current === sessionId
      )
        setReady(false);
      else if (currentAction()) setError((e as Error).message);
      return false;
    } finally {
      if (actionGeneration.current === actionId) setPending(false);
    }
  };
  const go = (r: Route) => {
    location.hash = r;
    setRoute(r);
    setSidebar(false);
    setSearch("");
  };
  return {
    ready,
    setReady,
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
    go,
    chooseBrand,
  };
}
