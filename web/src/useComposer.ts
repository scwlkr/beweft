import { useEffect, useRef, useState } from "react";
import {
  api,
  PostSchema,
  type Brand,
  type Channel,
  type Media,
  type Post,
} from "./api";
import type {
  CreatePostRequest,
  UpdatePostRequest,
} from "./lib/api-contract.generated";
import { localDateTime } from "./components/shared";
import {
  clearScheduleIntent,
  inspectScheduleIntent,
  makeScheduleIntent,
  readScheduleIntent,
  scheduleLock,
  sendScheduleIntent,
} from "./scheduleIntent";
import type { usePendingSchedule } from "./usePendingSchedule";
export function useComposer({
  brand,
  channels,
  media,
  initial,
  initialDate,
  onSaved,
  onClose,
  pendingSchedule,
}: {
  brand: Brand;
  channels: Channel[];
  media: Media[];
  initial: Partial<Post> | null;
  initialDate?: string;
  onSaved: () => Promise<void>;
  onClose: () => void;
  pendingSchedule: ReturnType<typeof usePendingSchedule>;
}) {
  const mounted = useRef(true);
  const running = useRef(false);
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
  const [requestedTab, setTab] = useState("all");
  const tab =
    requestedTab === "all" || selected.includes(requestedTab)
      ? requestedTab
      : "all";
  useEffect(() => {
    if (requestedTab !== tab) setTab(tab);
  }, [requestedTab, tab]);
  const [mediaOpen, setMediaOpen] = useState(false);
  const [mode, setMode] = useState<"draft" | "queue" | "exact">(
    initialDate ? "exact" : "draft",
  );
  const [date, setDate] = useState(initialDate ?? localDateTime());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [recovery, setRecovery] = useState<
    "unchecked" | "retry" | "release" | "partial" | "complete"
  >("unchecked");
  const [recoveryMessage, setRecoveryMessage] = useState("");
  const intent = pendingSchedule.intent;
  useEffect(() => {
    if (!intent) return;
    const saved = intent.post;
    setPost(saved);
    setTitle(saved.title);
    setText(saved.text);
    setSelected(intent.channel_ids);
    setMediaIds(saved.media_ids);
    setVariants(
      Object.fromEntries(
        saved.variants
          .filter((v) => v.text !== undefined)
          .map((v) => [v.channel_id, v.text!]),
      ),
    );
    setVariantMedia(
      Object.fromEntries(
        saved.variants
          .filter((v) => v.media_ids !== undefined)
          .map((v) => [v.channel_id, v.media_ids!]),
      ),
    );
    setOptions(
      Object.fromEntries(
        saved.variants.map((v) => [v.channel_id, v.options ?? {}]),
      ),
    );
    setMode(intent.mode);
    if (intent.scheduled_at) setDate(localDateTime(intent.scheduled_at));
    setRecovery("unchecked");
    setRecoveryMessage("");
    // Each stored intent represents an immutable post revision.
  }, [intent?.id]);
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
  const recover = async (_retry: boolean) => {
    if (running.current || !pendingSchedule.ready || !intent) return;
    running.current = true;
    setBusy(true);
    setError("");
    try {
      await scheduleLock(pendingSchedule.key, async () => {
        const saved = readScheduleIntent(pendingSchedule.key);
        if (!saved || saved.id !== intent.id)
          throw new Error(
            "This scheduling intent changed in another tab. Reopen it to review the current outcome.",
          );
        const state = await inspectScheduleIntent(saved);
        if (state.recorded.length) {
          if (mounted.current) {
            setRecovery("partial");
            setRecoveryMessage(
              "Matching publications exist, but cannot yet be tied to this request. Review the queue and keep this intent until request-specific recovery is available.",
            );
          }
        } else if (state.fenced || state.rejected) {
          if (mounted.current) {
            setRecovery("release");
            setRecoveryMessage(
              state.fenced
                ? "The post changed after this request. The original revision can no longer be scheduled; refresh the post before editing."
                : `The server rejected this request. ${saved.rejection ?? "Review the post before trying again."}`,
            );
          }
        } else if (mounted.current) {
          setRecovery("retry");
          setRecoveryMessage(
            "No publication was found yet. The original request could still complete; request-specific recovery is needed before retrying.",
          );
        }
      });
    } catch (e) {
      if (mounted.current)
        setError(
          `${e instanceof Error ? e.message : "Unable to check scheduling."} The original schedule is still saved for recovery.`,
        );
    } finally {
      running.current = false;
      if (mounted.current) setBusy(false);
    }
  };
  const release = async () => {
    if (running.current || !pendingSchedule.ready || !intent) return;
    running.current = true;
    setBusy(true);
    setError("");
    try {
      await scheduleLock(pendingSchedule.key, async () => {
        const saved = readScheduleIntent(pendingSchedule.key);
        if (!saved || saved.id !== intent.id)
          throw new Error("Scheduling intent changed in another tab.");
        const state = await inspectScheduleIntent(saved);
        if (state.recorded.length || !(state.fenced || state.rejected))
          throw new Error(
            "The original outcome is not yet safe to release. Check it again.",
          );
        clearScheduleIntent(pendingSchedule.key, saved.id);
        if (mounted.current) {
          await onSaved();
          if (mounted.current) onClose();
        }
      });
    } catch (e) {
      if (mounted.current)
        setError(
          e instanceof Error ? e.message : "Unable to release the schedule.",
        );
    } finally {
      running.current = false;
      if (mounted.current) setBusy(false);
    }
  };
  const submit = async () => {
    if (
      running.current ||
      !pendingSchedule.ready ||
      intent ||
      recovery === "complete"
    )
      return;
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
    running.current = true;
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
      await scheduleLock(pendingSchedule.key, async () => {
        if (readScheduleIntent(pendingSchedule.key))
          throw new Error(
            "Another tab has an unfinished schedule. Review that original schedule before saving a post.",
          );
        const saved = await api(
          post?.id ? `/posts/${post.id}` : "/posts",
          PostSchema,
          post?.id ? "PATCH" : "POST",
          post?.id
            ? ({ ...body, version: post.version! } satisfies UpdatePostRequest)
            : ({ ...body, brand_id: brand.id } satisfies CreatePostRequest),
        );
        if (mounted.current) setPost(saved);
        if (!mounted.current) return;
        if (mode !== "draft") {
          const schedule = makeScheduleIntent(saved, selected, mode, date);
          const result = await sendScheduleIntent(
            pendingSchedule.key,
            schedule,
          );
          if (
            !schedule.channel_ids.every((id) =>
              result.some(
                (p) =>
                  p.channel_id === id &&
                  p.post_id === saved.id &&
                  p.post_version === saved.version,
              ),
            )
          )
            throw new Error(
              "The scheduling response was incomplete. Check the original outcome before trying again.",
            );
          clearScheduleIntent(pendingSchedule.key, schedule.id);
        }
        if (mounted.current) {
          await onSaved();
          if (mounted.current) onClose();
        }
      });
    } catch (e) {
      if (mounted.current)
        setError(e instanceof Error ? e.message : "Unable to save your post.");
    } finally {
      running.current = false;
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
    intent,
    recovery,
    recoveryMessage,
    recover,
    release,
    active,
    currentText,
    previewChannel,
    activeMedia,
    setActiveMedia,
    selectedMedia,
    submit,
  };
}
