import { useEffect, useRef, useState } from "react";
import { z } from "zod";
import {
  api,
  PostSchema,
  PublicationSchema,
  type Brand,
  type Channel,
  type Media,
  type Post,
} from "./api";
import type {
  CreatePostRequest,
  UpdatePostRequest,
  SchedulePostRequest,
} from "./lib/api-contract.generated";
import { localDateTime } from "./components/shared";
export function useComposer({
  brand,
  channels,
  media,
  initial,
  initialDate,
  onSaved,
  onClose,
}: {
  brand: Brand;
  channels: Channel[];
  media: Media[];
  initial: Partial<Post> | null;
  initialDate?: string;
  onSaved: () => Promise<void>;
  onClose: () => void;
}) {
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const [post, setPost] = useState(initial);
  const [title, setTitle] = useState(initial?.title ?? "");
  const [text, setText] = useState(initial?.text ?? "");
  const [selected, setSelected] = useState<string[]>(
    initial?.variants?.map((v) => v.channel_id) ?? [],
  );
  const [mediaIds, setMediaIds] = useState<string[]>(initial?.media_ids ?? []);
  const [variants, setVariants] = useState<Record<string, string>>(
    Object.fromEntries(
      initial?.variants
        ?.filter((v) => v.text !== undefined)
        .map((v) => [v.channel_id, v.text!]) ?? [],
    ),
  );
  const [variantMedia, setVariantMedia] = useState<Record<string, string[]>>(
    Object.fromEntries(
      initial?.variants
        ?.filter((v) => v.media_ids !== undefined)
        .map((v) => [v.channel_id, v.media_ids!]) ?? [],
    ),
  );
  const [options, setOptions] = useState<
    Record<string, Record<string, unknown>>
  >(
    Object.fromEntries(
      initial?.variants?.map((v) => [v.channel_id, v.options ?? {}]) ?? [],
    ),
  );
  const [tab, setTab] = useState("all");
  const [mediaOpen, setMediaOpen] = useState(false);
  const [mode, setMode] = useState<"draft" | "queue" | "exact">(
    initialDate ? "exact" : "draft",
  );
  const [date, setDate] = useState(initialDate ?? localDateTime());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const active = channels.find((c) => c.id === tab);
  const currentText = tab === "all" ? text : (variants[tab] ?? text);
  const previewChannel = active;
  const activeMedia =
    tab === "all" ? mediaIds : (variantMedia[tab] ?? mediaIds);
  const setActiveMedia = (ids: string[]) =>
    tab === "all"
      ? setMediaIds(ids)
      : setVariantMedia({ ...variantMedia, [tab]: ids });
  const selectedMedia = media.filter((m) => activeMedia.includes(m.id));
  const submit = async () => {
    setError("");
    const hasContent = (id: string) =>
      (variants[id] ?? text).trim().length > 0 ||
      (variantMedia[id] ?? mediaIds).length > 0;
    if (
      selected.length
        ? !selected.some(hasContent)
        : !text.trim() && !mediaIds.length
    ) {
      setError("Add some text or media before saving your post.");
      return;
    }
    if (mode !== "draft" && selected.some((id) => !hasContent(id))) {
      setError(
        "Add text or media for each selected channel before scheduling.",
      );
      return;
    }
    if (mode !== "draft" && !selected.length) {
      setError("Choose at least one channel to schedule this post.");
      return;
    }
    if (mode === "exact" && (!date || new Date(date).getTime() < Date.now())) {
      setError("Choose a future date and time.");
      return;
    }
    if (mode !== "draft") {
      const youtube = channels.find(
        (c) =>
          selected.includes(c.id) &&
          c.provider === "youtube" &&
          (!options[c.id]?.privacy ||
            typeof options[c.id]?.made_for_kids !== "boolean"),
      );
      if (youtube) {
        setTab(youtube.id);
        setError("Choose YouTube privacy and audience before scheduling.");
        return;
      }
    }
    setBusy(true);
    try {
      const body = {
        title,
        text,
        media_ids: mediaIds,
        options: initial?.options ?? {},
        variants: selected.map((channel_id) => ({
          channel_id,
          options: options[channel_id] ?? {},
          ...(variantMedia[channel_id] !== undefined
            ? { media_ids: variantMedia[channel_id] }
            : {}),
          ...(variants[channel_id] !== undefined
            ? { text: variants[channel_id] }
            : {}),
        })),
      };
      const saved = await api(
        post?.id ? `/posts/${post.id}` : "/posts",
        PostSchema,
        post?.id ? "PATCH" : "POST",
        post?.id
          ? ({ ...body, version: post.version! } satisfies UpdatePostRequest)
          : ({ ...body, brand_id: brand.id } satisfies CreatePostRequest),
      );
      if (mounted.current) setPost(saved);
      if (mode !== "draft")
        await api(
          `/posts/${saved.id}/schedule`,
          z.array(PublicationSchema),
          "POST",
          {
            channel_ids: selected,
            version: saved.version,
            ...(mode === "exact"
              ? { scheduled_at: new Date(date).toISOString() }
              : {}),
          } satisfies SchedulePostRequest,
        );
      if (mounted.current) {
        await onSaved();
        if (mounted.current) onClose();
      }
    } catch (e) {
      if (mounted.current)
        setError(e instanceof Error ? e.message : "Unable to save your post.");
    } finally {
      if (mounted.current) setBusy(false);
    }
  };
  return {
    post,
    title,
    setTitle,
    text,
    setText,
    selected,
    setSelected,
    mediaIds,
    setMediaIds,
    variants,
    setVariants,
    variantMedia,
    setVariantMedia,
    options,
    setOptions,
    tab,
    setTab,
    mediaOpen,
    setMediaOpen,
    mode,
    setMode,
    date,
    setDate,
    busy,
    error,
    active,
    currentText,
    previewChannel,
    activeMedia,
    setActiveMedia,
    selectedMedia,
    submit,
  };
}
