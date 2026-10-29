import { useEffect, useRef, useState } from "react";
import {
  api,
  captureWorkspaceSession,
  isCurrentWorkspaceSession,
  requireWorkspaceSession,
  PostSchema,
  type ApiSession,
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
  sameScheduleIntent,
  scheduleLock,
  sendScheduleIntent,
  type PublicationSummary,
} from "./scheduleIntent";
import type { usePendingSchedule } from "./usePendingSchedule";
function confirmationMessage(rows: PublicationSummary[]) {
  const canceled = rows.filter((row) => row.state === "canceled").length;
  return `The original request is confirmed for ${rows.length} ${rows.length === 1 ? "channel" : "channels"}.${
    canceled
      ? ` ${canceled} ${canceled === 1 ? "item is" : "items are"} now canceled.`
      : ""
  }`;
}
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
  const lifetime = useRef<AbortController | null>(null);
  useEffect(() => {
    mounted.current = true;
    const controller = new AbortController();
    lifetime.current = controller;
    return () => {
      mounted.current = false;
      controller.abort();
      if (lifetime.current === controller) lifetime.current = null;
    };
  }, []);
  const active = (session: ApiSession, signal?: AbortSignal) => {
    if (
      !mounted.current ||
      !lifetime.current ||
      lifetime.current.signal !== signal ||
      signal?.aborted
    )
      return false;
    requireWorkspaceSession(session);
    return true;
  };
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
    | "unchecked"
    | "retry"
    | "release"
    | "partial"
    | "mismatch"
    | "legacy"
    | "complete"
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
  const activeChannel = channels.find((c) => c.id === tab);
  const currentText = tab === "all" ? text : (variants[tab] ?? text);
  const previewChannel = activeChannel;
  const activeMedia =
    tab === "all" ? mediaIds : (variantMedia[tab] ?? mediaIds);
  const setActiveMedia = (ids: string[]) =>
    tab === "all"
      ? setMediaIds(ids)
      : setVariantMedia({ ...variantMedia, [tab]: ids });
  const selectedMedia = media.filter((m) => activeMedia.includes(m.id));
  const recover = async (retry: boolean) => {
    if (running.current || !pendingSchedule.ready || !intent) return;
    let session: ApiSession;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    const signal = lifetime.current?.signal;
    running.current = true;
    setBusy(true);
    setError("");
    try {
      await scheduleLock(
        pendingSchedule.key,
        async (lockedSession) => {
          if (!active(lockedSession, signal)) return;
          const saved = readScheduleIntent(pendingSchedule.key);
          if (!saved || !sameScheduleIntent(saved, intent))
            throw new Error(
              "This scheduling intent changed in another tab. Reopen it to review the current outcome.",
            );
          if (retry) {
            if (saved.request_contract !== "request-id-v1" || !saved.uncertain)
              throw new Error(
                "Only an unresolved request with its original request ID can be retried.",
              );
            const rows = await sendScheduleIntent(
              pendingSchedule.key,
              saved,
              lockedSession,
              true,
              signal,
            );
            if (!active(lockedSession, signal)) return;
            await clearScheduleIntent(
              pendingSchedule.key,
              saved.id,
              lockedSession,
              signal,
            );
            if (!active(lockedSession, signal)) return;
            setRecovery("complete");
            setRecoveryMessage(confirmationMessage(rows));
            await onSaved();
            return;
          }
          const state = await inspectScheduleIntent(
            saved,
            lockedSession,
            signal,
          );
          if (!active(lockedSession, signal)) return;
          if (state.kind === "complete") {
            await clearScheduleIntent(
              pendingSchedule.key,
              saved.id,
              lockedSession,
              signal,
            );
            if (!active(lockedSession, signal)) return;
            setRecovery("complete");
            setRecoveryMessage(confirmationMessage(state.publications));
            await onSaved();
          } else if (state.kind === "legacy") {
            setRecovery("legacy");
            setRecoveryMessage(
              "This older request was sent without a recovery ID. Check the original destinations and keep this record; retrying could create a duplicate.",
            );
          } else if (state.kind === "mismatch") {
            setRecovery("mismatch");
            setRecoveryMessage(
              "The server record does not match this saved request. The original remains locked for review.",
            );
          } else if (state.kind === "partial") {
            setRecovery("partial");
            setRecoveryMessage(
              "The server returned only part of this request. The original remains saved; check again or retry the same request ID.",
            );
          } else if (!saved.uncertain && saved.rejection) {
            setRecovery("release");
            setRecoveryMessage(
              `The original request was rejected: ${saved.rejection}`,
            );
          } else {
            setRecovery("retry");
            setRecoveryMessage(
              "No request record is visible yet. The original may still complete; an explicit retry uses the same request ID and revision.",
            );
          }
        },
        signal,
        session,
        true,
      );
    } catch (e) {
      if (
        mounted.current &&
        lifetime.current?.signal === signal &&
        isCurrentWorkspaceSession(session)
      )
        setError(
          `${e instanceof Error ? e.message : "Unable to check scheduling."} The original schedule is still saved for recovery.`,
        );
    } finally {
      running.current = false;
      if (
        mounted.current &&
        lifetime.current?.signal === signal &&
        isCurrentWorkspaceSession(session)
      )
        setBusy(false);
    }
  };
  const release = async () => {
    if (running.current || !pendingSchedule.ready || !intent) return;
    let session: ApiSession;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    const signal = lifetime.current?.signal;
    running.current = true;
    setBusy(true);
    setError("");
    try {
      await scheduleLock(
        pendingSchedule.key,
        async (lockedSession) => {
          if (!active(lockedSession, signal)) return;
          const saved = readScheduleIntent(pendingSchedule.key);
          if (!saved || !sameScheduleIntent(saved, intent))
            throw new Error("Scheduling intent changed in another tab.");
          if (
            saved.request_contract !== "request-id-v1" ||
            saved.uncertain ||
            !saved.rejection
          )
            throw new Error(
              "The original outcome is not yet safe to release. Check it again.",
            );
          const state = await inspectScheduleIntent(
            saved,
            lockedSession,
            signal,
          );
          if (!active(lockedSession, signal)) return;
          if (state.kind !== "missing")
            throw new Error(
              "A server record exists or could not be verified. Check the original outcome again.",
            );
          await clearScheduleIntent(
            pendingSchedule.key,
            saved.id,
            lockedSession,
            signal,
          );
          if (!active(lockedSession, signal)) return;
          await onSaved();
          if (active(lockedSession, signal)) onClose();
        },
        signal,
        session,
        true,
      );
    } catch (e) {
      if (
        mounted.current &&
        lifetime.current?.signal === signal &&
        isCurrentWorkspaceSession(session)
      )
        setError(
          e instanceof Error ? e.message : "Unable to release the schedule.",
        );
    } finally {
      running.current = false;
      if (
        mounted.current &&
        lifetime.current?.signal === signal &&
        isCurrentWorkspaceSession(session)
      )
        setBusy(false);
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
    let session: ApiSession;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    const signal = lifetime.current?.signal;
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
      await scheduleLock(
        pendingSchedule.key,
        async (lockedSession) => {
          if (!active(lockedSession, signal)) return;
          if (readScheduleIntent(pendingSchedule.key))
            throw new Error(
              "Another tab has an unfinished schedule. Review that original schedule before saving a post.",
            );
          const saved = await api(
            post?.id ? `/posts/${post.id}` : "/posts",
            PostSchema,
            post?.id ? "PATCH" : "POST",
            post?.id
              ? ({
                  ...body,
                  version: post.version!,
                } satisfies UpdatePostRequest)
              : ({ ...body, brand_id: brand.id } satisfies CreatePostRequest),
          );
          if (!active(lockedSession, signal)) return;
          setPost(saved);
          if (mode !== "draft") {
            const schedule = makeScheduleIntent(saved, selected, mode, date);
            await sendScheduleIntent(
              pendingSchedule.key,
              schedule,
              lockedSession,
              false,
              signal,
            );
            if (!active(lockedSession, signal)) return;
            await clearScheduleIntent(
              pendingSchedule.key,
              schedule.id,
              lockedSession,
              signal,
            );
          }
          if (!active(lockedSession, signal)) return;
          await onSaved();
          if (active(lockedSession, signal)) onClose();
        },
        signal,
        session,
        true,
      );
    } catch (e) {
      if (
        mounted.current &&
        lifetime.current?.signal === signal &&
        isCurrentWorkspaceSession(session)
      )
        setError(e instanceof Error ? e.message : "Unable to save your post.");
    } finally {
      running.current = false;
      if (
        mounted.current &&
        lifetime.current?.signal === signal &&
        isCurrentWorkspaceSession(session)
      )
        setBusy(false);
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
    active: activeChannel,
    currentText,
    previewChannel,
    activeMedia,
    setActiveMedia,
    selectedMedia,
    submit,
  };
}
