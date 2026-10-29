import { useEffect, useRef, useState } from "react";
import {
  api,
  ApiError,
  captureWorkspaceSession,
  isCurrentWorkspaceSession,
  requireWorkspaceSession,
  PostSchema,
  MediaSchema,
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
  effectiveMediaIds,
  inspectScheduleIntent,
  makeScheduleIntent,
  markScheduleMismatch,
  readScheduleIntent,
  requireSavedSchedule,
  retireScheduleIntent,
  sameScheduleIntent,
  scheduleHasConflict,
  scheduleLock,
  sendScheduleIntent,
  type PublicationSummary,
  type ScheduleIntent,
} from "./scheduleIntent";
import type { usePendingSchedule } from "./usePendingSchedule";
import { withMediaScheduleGuard } from "./mediaIntent";
import type { PendingMedia } from "./usePendingMedia";
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
  pendingMedia,
  reconcileMedia,
}: {
  brand: Brand;
  channels: Channel[];
  media: Media[];
  initial: Partial<Post> | null;
  initialDate?: string;
  onSaved: () => Promise<void>;
  onClose: () => void;
  pendingSchedule: ReturnType<typeof usePendingSchedule>;
  pendingMedia: PendingMedia;
  reconcileMedia: (media: Media, session: ApiSession) => void;
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
  const openedIntent = useRef<ScheduleIntent | null>(null);
  if (!intent) openedIntent.current = null;
  else if (!openedIntent.current || openedIntent.current.id !== intent.id)
    openedIntent.current = intent;
  const intentChanged =
    !!intent &&
    !!openedIntent.current &&
    !sameScheduleIntent(openedIntent.current, intent);
  useEffect(() => {
    if (!intentChanged) return;
    setRecovery("mismatch");
    setRecoveryMessage(
      "The saved schedule changed in another tab. Close and reopen it to review the current record.",
    );
  }, [intentChanged]);
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
    if (intent.rejection_code === "schedule_request_conflict") {
      setRecovery("mismatch");
      setRecoveryMessage(
        "The server record does not match this saved request. The original remains locked for review.",
      );
    } else if (
      intent.rejection_code === "media_version_conflict" &&
      !intent.uncertain
    ) {
      setRecovery("release");
      setRecoveryMessage(
        "Media descriptions changed. Refresh them and review before scheduling again.",
      );
    } else {
      setRecovery("unchecked");
      setRecoveryMessage("");
    }
    // Each stored intent represents an immutable post revision.
  }, [intent?.id]);
  useEffect(() => {
    if (intentChanged) return;
    if (intent?.rejection_code === "schedule_request_conflict") {
      setRecovery("mismatch");
      setRecoveryMessage(
        "The server record does not match this saved request. The original remains locked for review.",
      );
    } else if (
      intent?.rejection_code === "media_version_conflict" &&
      !intent.uncertain
    ) {
      setRecovery("release");
      setRecoveryMessage(
        "Media descriptions changed. Refresh them and review before scheduling again.",
      );
    }
  }, [intent?.id, intent?.rejection_code, intent?.uncertain, intentChanged]);
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
  const scheduledMediaIds = [
    ...new Set(selected.flatMap((id) => variantMedia[id] ?? mediaIds)),
  ];
  const affectedMedia = scheduledMediaIds.filter(
    (id) =>
      !!pendingMedia.intents[id] ||
      pendingMedia.checking.includes(id) ||
      !!pendingMedia.assetErrors[id],
  );
  const missingMedia = scheduledMediaIds.filter(
    (id) => !media.some((item) => item.id === id),
  );
  const scheduleBlocked =
    mode !== "draft" &&
    (!pendingMedia.ready ||
      affectedMedia.length > 0 ||
      missingMedia.length > 0);
  const hasDefiniteMediaConflict = () => {
    try {
      const saved = readScheduleIntent(pendingSchedule.key);
      return (
        saved?.rejection_code === "media_version_conflict" && !saved.uncertain
      );
    } catch {
      return false;
    }
  };
  const settleRejection = async (
    saved: ScheduleIntent,
    rejection: { code: string; message: string },
    lockedSession: ApiSession,
    signal?: AbortSignal,
  ) => {
    const current = await requireSavedSchedule(
      pendingSchedule.key,
      saved,
      lockedSession,
      signal,
    );
    if (!active(lockedSession, signal)) return;
    const mediaChanged = current.rejection_code === "media_version_conflict";
    let draftMissing = false;
    try {
      const latest = await api(
        `/posts/${saved.post.id}`,
        PostSchema,
        "GET",
        undefined,
        signal,
      );
      if (
        latest.id !== saved.post.id ||
        latest.brand_id !== saved.post.brand_id
      )
        throw new Error("The draft check returned a different post.");
    } catch (failure) {
      if (failure instanceof ApiError && failure.status === 404)
        draftMissing = true;
      else throw failure;
    }
    if (!active(lockedSession, signal)) return;
    await requireSavedSchedule(
      pendingSchedule.key,
      saved,
      lockedSession,
      signal,
    );
    if (!active(lockedSession, signal)) return;
    for (const id of Object.keys(saved.media_versions ?? {})) {
      try {
        const row = await api(
          `/media/${id}`,
          MediaSchema,
          "GET",
          undefined,
          signal,
        );
        if (!active(lockedSession, signal)) return;
        await requireSavedSchedule(
          pendingSchedule.key,
          saved,
          lockedSession,
          signal,
        );
        if (!active(lockedSession, signal)) return;
        if (row.id !== id || row.brand_id !== saved.post.brand_id)
          throw new Error("The media check returned a different asset.");
        reconcileMedia(row, lockedSession);
      } catch (failure) {
        if (!(failure instanceof ApiError && failure.status === 404))
          throw failure;
      }
    }
    await requireSavedSchedule(
      pendingSchedule.key,
      saved,
      lockedSession,
      signal,
    );
    if (!active(lockedSession, signal)) return;
    await onSaved();
    if (!active(lockedSession, signal)) return;
    await clearScheduleIntent(
      pendingSchedule.key,
      saved,
      { kind: "rejected" },
      lockedSession,
      signal,
    );
    if (!active(lockedSession, signal)) return;
    if (draftMissing) setPost(null);
    setRecovery("unchecked");
    setRecoveryMessage("");
    setError(
      mediaChanged
        ? "Media descriptions changed. Review the refreshed descriptions before scheduling again."
        : rejection.code === "schedule_request_retired"
          ? "The original schedule was stopped. You can make changes and schedule again."
          : `The original schedule was not made: ${rejection.message} You can correct it and schedule again.`,
    );
    if (draftMissing)
      setError(
        "The original schedule was not made and the draft was deleted. Save this content as a new draft or schedule it again.",
      );
  };
  const recover = async (retry: boolean) => {
    if (running.current || !pendingSchedule.ready || !intent || intentChanged)
      return;
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
          if (scheduleHasConflict(saved))
            throw new ApiError(
              "The server record does not match this saved request.",
              409,
              "schedule_request_conflict",
            );
          if (retry) {
            if (
              saved.request_contract !== "request-id-v2" ||
              (saved.rejection_code === "media_version_conflict" &&
                !saved.uncertain)
            )
              throw new Error(
                "Only an unresolved original schedule can be retried.",
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
              saved,
              { kind: "accepted", publicationIds: rows.map((row) => row.id) },
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
              saved,
              {
                kind: "accepted",
                publicationIds: state.publications.map((row) => row.id),
              },
              lockedSession,
              signal,
            );
            if (!active(lockedSession, signal)) return;
            setRecovery("complete");
            setRecoveryMessage(confirmationMessage(state.publications));
            await onSaved();
          } else if (state.kind === "mismatch") {
            await markScheduleMismatch(
              pendingSchedule.key,
              saved,
              lockedSession,
            );
            if (!active(lockedSession, signal)) return;
            setRecovery("mismatch");
            setRecoveryMessage(
              "The server record does not match this saved request. The original remains locked for review.",
            );
          } else if (state.kind === "rejected") {
            await settleRejection(saved, state.error, lockedSession, signal);
          } else if (
            state.kind === "legacy" ||
            saved.request_contract !== "request-id-v2"
          ) {
            setRecovery("legacy");
            setRecoveryMessage(
              saved.request_contract === "request-id-v1"
                ? "This older request has no saved media versions. Keep its record and check the original outcome; retrying is not safe."
                : "This older request was sent without a recovery ID. Check the original destinations and keep this record; retrying could create a duplicate.",
            );
          } else if (state.kind === "partial") {
            setRecovery("partial");
            setRecoveryMessage(
              "The server returned only part of this request. The original remains saved; check again or retry the original schedule.",
            );
          } else {
            setRecovery("retry");
            setRecoveryMessage(
              "No request record is visible yet. The original may still complete; retrying uses the same saved schedule.",
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
      ) {
        if (e instanceof ApiError && e.code === "schedule_request_conflict") {
          setRecovery("mismatch");
          setRecoveryMessage(
            "The server record does not match this saved request. The original remains locked for review.",
          );
        } else if (
          e instanceof ApiError &&
          e.code === "media_version_conflict" &&
          hasDefiniteMediaConflict()
        ) {
          setRecovery("release");
          setRecoveryMessage(
            "Media descriptions changed. Refresh them and review before scheduling again.",
          );
        }
        setError(
          `${e instanceof Error ? e.message : "Unable to check scheduling."} The original schedule is still saved for recovery.`,
        );
      }
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
    if (running.current || !pendingSchedule.ready || !intent || intentChanged)
      return;
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
            saved.request_contract !== "request-id-v2" ||
            scheduleHasConflict(saved)
          )
            throw new Error(
              "This original schedule cannot be stopped safely here.",
            );
          const state = await retireScheduleIntent(
            saved,
            lockedSession,
            signal,
          );
          if (!active(lockedSession, signal)) return;
          if (state.kind === "complete") {
            await clearScheduleIntent(
              pendingSchedule.key,
              saved,
              {
                kind: "accepted",
                publicationIds: state.publications.map((row) => row.id),
              },
              lockedSession,
              signal,
            );
            if (!active(lockedSession, signal)) return;
            setRecovery("complete");
            setRecoveryMessage(confirmationMessage(state.publications));
            await onSaved();
          } else if (state.kind === "rejected") {
            await settleRejection(saved, state.error, lockedSession, signal);
          } else if (state.kind === "mismatch") {
            await markScheduleMismatch(
              pendingSchedule.key,
              saved,
              lockedSession,
            );
            throw new ApiError(
              "The server record does not match this saved request.",
              409,
              "schedule_request_conflict",
            );
          } else {
            throw new Error(
              "The original outcome could not be fully confirmed. Check it again.",
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
      ) {
        if (e instanceof ApiError && e.code === "schedule_request_conflict") {
          await markScheduleMismatch(
            pendingSchedule.key,
            intent,
            session,
          ).catch(() => {});
          if (
            !mounted.current ||
            lifetime.current?.signal !== signal ||
            signal?.aborted ||
            !isCurrentWorkspaceSession(session)
          )
            return;
          setRecovery("mismatch");
          setRecoveryMessage(
            "The server record does not match this saved request. The original remains locked for review.",
          );
        }
        setError(
          e instanceof Error ? e.message : "Unable to stop the schedule.",
        );
      }
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
      recovery === "complete" ||
      scheduleBlocked
    )
      return;
    if (
      mode !== "draft" &&
      (!pendingMedia.session ||
        !pendingMedia.owner ||
        pendingMedia.owner !== pendingSchedule.owner ||
        !isCurrentWorkspaceSession(pendingMedia.session))
    ) {
      setError("Media descriptions must be checked before scheduling.");
      return;
    }
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
          const snapshots = scheduledMediaIds.map(
            (id) => media.find((item) => item.id === id)!,
          );
          if (mode !== "draft") {
            if (!signal)
              throw new Error(
                "Media descriptions must be checked before scheduling.",
              );
            await withMediaScheduleGuard(
              pendingMedia.owner,
              brand.id,
              snapshots,
              lockedSession,
              signal,
              reconcileMedia,
              async () => undefined,
            );
            if (!active(lockedSession, signal)) return;
          }
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
            if (!signal)
              throw new Error(
                "Media descriptions must be checked before scheduling.",
              );
            const effective = effectiveMediaIds(saved, selected);
            if (
              JSON.stringify(effective) !==
              JSON.stringify([...scheduledMediaIds].sort())
            )
              throw new Error(
                "The saved media selection changed. Refresh and review it before scheduling.",
              );
            const mediaVersions = await withMediaScheduleGuard(
              pendingMedia.owner,
              brand.id,
              snapshots,
              lockedSession,
              signal,
              reconcileMedia,
              async (latest) =>
                Object.fromEntries(
                  latest.map((item) => [item.id, item.metadata_version]),
                ),
            );
            if (!active(lockedSession, signal)) return;
            const schedule = makeScheduleIntent(
              saved,
              pendingSchedule.owner,
              selected,
              mode,
              date,
              mediaVersions,
            );
            const rows = await sendScheduleIntent(
              pendingSchedule.key,
              schedule,
              lockedSession,
              false,
              signal,
            );
            if (!active(lockedSession, signal)) return;
            await clearScheduleIntent(
              pendingSchedule.key,
              schedule,
              { kind: "accepted", publicationIds: rows.map((row) => row.id) },
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
      ) {
        if (e instanceof ApiError && e.code === "schedule_request_conflict") {
          setRecovery("mismatch");
          setRecoveryMessage(
            "The server record does not match this saved request. The original remains locked for review.",
          );
        } else if (
          e instanceof ApiError &&
          e.code === "media_version_conflict" &&
          hasDefiniteMediaConflict()
        ) {
          setRecovery("release");
          setRecoveryMessage(
            "Media descriptions changed. Refresh them and review before scheduling again.",
          );
        }
        setError(e instanceof Error ? e.message : "Unable to save your post.");
      }
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
    intentChanged,
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
    affectedMedia,
    missingMedia,
    scheduleBlocked,
    submit,
  };
}
