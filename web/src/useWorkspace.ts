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
export function useWorkspace() {
  const [ready, setReady] = useState<boolean | null>(null);
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
  const generation = useRef(0);
  const context = useRef(brandId);
  context.current = brandId;
  const brand = brands.find((b) => b.id === brandId);
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
  }, []);
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
  const load = useCallback(async () => {
    if (!ready || context.current !== brandId) return;
    const current = ++generation.current;
    setLoading(true);
    try {
      const [bs, ps, h] = await Promise.all([
        api("/brands", z.array(BrandSchema)),
        api("/providers", z.array(ProviderSchema)),
        api("/jobs/health", HealthSchema),
      ]);
      if (current !== generation.current) return;
      setBrands(bs);
      setProviders(ps);
      setHealth(h);
      const next = bs.some((b) => b.id === brandId)
        ? brandId
        : (bs[0]?.id ?? "");
      if (next !== brandId) {
        setBrandId(next);
        return;
      }
      if (next) {
        const q = `?brand_id=${encodeURIComponent(next)}`;
        const [cs, posts, ins, ms, pubs] = await Promise.all([
          api(`/channels${q}`, z.array(ChannelSchema)),
          api(`/posts${q}`, z.array(PostSchema)),
          api(`/ideas${q}`, z.array(IdeaSchema)),
          api(`/media${q}`, z.array(MediaSchema)),
          api(`/publications${q}`, z.array(PublicationSchema)),
        ]);
        if (current !== generation.current) return;
        setChannels(cs);
        setPosts(posts);
        setIdeas(ins);
        setMedia(ms);
        setPublications(pubs);
      } else {
        setChannels([]);
        setPosts([]);
        setIdeas([]);
        setMedia([]);
        setPublications([]);
      }
      setError("");
    } catch (e) {
      if (current === generation.current) {
        if (e instanceof ApiError && e.status === 401) setReady(false);
        else setError((e as Error).message);
      }
    } finally {
      if (current === generation.current) setLoading(false);
    }
  }, [ready, brandId]);
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
    setPending(true);
    setError("");
    try {
      await fn();
      if (context.current !== brandId) return false;
      await load();
      if (context.current !== brandId) return false;
      setToast(message);
      return true;
    } catch (e) {
      if (context.current === brandId) setError((e as Error).message);
      return false;
    } finally {
      setPending(false);
    }
  };
  const go = (r: Route) => {
    location.hash = r;
    setRoute(r);
    setSidebar(false);
    setSearch("");
  };
  const chooseBrand = (id: string) => {
    ++generation.current;
    context.current = id;
    setError("");
    setToast("");
    setChannels([]);
    setPosts([]);
    setIdeas([]);
    setMedia([]);
    setPublications([]);
    setComposer(undefined);
    setBrandId(id);
    localStorage.setItem("beweft.brand", id);
    setSidebar(false);
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
    error,
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
