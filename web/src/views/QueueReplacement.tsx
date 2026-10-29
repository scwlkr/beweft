import { useEffect, useRef, useState } from "react";
import { z } from "zod";
import {
  api,
  ApiError,
  captureWorkspaceSession,
  isCurrentWorkspaceSession,
  PublicationSchema,
  type ApiSession,
  type Media,
  type Post,
  type Publication,
} from "../api";
import { ErrorNotice } from "../components/shared";
import { Button } from "../components/ui/button";
import { withMediaScheduleGuard } from "../mediaIntent";
import {
  clearReplacement,
  matchesReplacement,
  readReplacement,
  recordReplacementRejection,
  saveReplacement,
  type CurrentReplacementIntent,
} from "../replacementIntent";
import { readScheduleIntent, scheduleLock } from "../scheduleIntent";
import type { PendingMedia } from "../usePendingMedia";
import type { usePendingSchedule } from "../usePendingSchedule";
type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;
export function QueueReplacement({
  publication,
  post,
  media,
  action,
  pending,
  pendingMedia,
  pendingSchedule,
  reconcileMedia,
  onEdit,
  onRefresh,
  onClose,
}: {
  publication: Publication;
  post?: Post;
  media: Media[];
  action: Action;
  pending: boolean;
  pendingMedia: PendingMedia;
  pendingSchedule: ReturnType<typeof usePendingSchedule>;
  reconcileMedia: (media: Media, session: ApiSession) => void;
  onEdit: (post: Post) => void;
  onRefresh: () => Promise<void>;
  onClose: () => void;
}) {
  const [review, setReview] = useState(false);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const running = useRef(false);
  const lifetime = useRef<AbortController | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    lifetime.current = controller;
    return () => {
      controller.abort();
      lifetime.current = null;
    };
  }, []);
  if (!post || publication.state !== "queued") return null;
  const variant = post.variants.find(
    (row) => row.channel_id === publication.channel_id,
  );
  const newMedia = variant?.media_ids ?? post.media_ids;
  const newText = variant?.text ?? post.text;
  const newOptions = variant?.options ?? post.options ?? {};
  const oldOptions = publication.payload.options ?? {};
  const replace = async () => {
    if (
      running.current ||
      pending ||
      !pendingSchedule.ready ||
      !pendingMedia.ready ||
      pendingMedia.brand !== post.brand_id ||
      !publication.scheduling_mode ||
      pendingSchedule.intent
    )
      return;
    const signal = lifetime.current?.signal;
    if (!signal) return;
    let session;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    running.current = true;
    setBusy(true);
    setMessage("");
    const intent: CurrentReplacementIntent = {
      contract: "replace-v1",
      publicationId: publication.id,
      postId: post.id,
      brandId: post.brand_id,
      channelId: publication.channel_id,
      publicationPostVersion: publication.post_version,
      postVersion: post.version,
      scheduledAt: publication.scheduled_at,
      schedulingMode: publication.scheduling_mode,
    };
    try {
      const done = await action(
        () =>
          scheduleLock(
            pendingSchedule.key,
            async (lockedSession) => {
              if (
                readScheduleIntent(pendingSchedule.key) ||
                readReplacement(pendingSchedule.key)
              )
                throw new Error(
                  "Another scheduling outcome needs review before replacing this publication.",
                );
              const snapshots = newMedia.map((id) =>
                media.find(
                  (item) => item.id === id && item.brand_id === post.brand_id,
                ),
              );
              if (snapshots.some((item) => !item))
                throw new Error(
                  "Some media is unavailable. Refresh and review the draft before replacing.",
                );
              await withMediaScheduleGuard(
                pendingMedia.owner,
                post.brand_id,
                snapshots as Media[],
                lockedSession,
                signal,
                reconcileMedia,
                async () => {
                  signal.throwIfAborted();
                  saveReplacement(pendingSchedule.key, intent);
                  try {
                    const result = await api(
                      `/publications/${publication.id}/replace`,
                      z.object({
                        previous_publication_id: z.string(),
                        publication: PublicationSchema,
                      }),
                      "POST",
                      {
                        post_version: post.version,
                        publication_post_version: publication.post_version,
                        scheduled_at: publication.scheduled_at,
                      },
                      signal,
                    );
                    if (
                      result.previous_publication_id !== intent.publicationId ||
                      !matchesReplacement(intent, result.publication)
                    )
                      throw new Error(
                        "Replacement response did not match the original publication.",
                      );
                    clearReplacement(pendingSchedule.key, intent);
                  } catch (failure) {
                    if (
                      failure instanceof ApiError &&
                      ((failure.status === 409 &&
                        failure.code === "conflict") ||
                        (failure.status === 400 &&
                          [
                            "invalid_request",
                            "provider_format_invalid",
                          ].includes(failure.code)) ||
                        (failure.status === 403 &&
                          failure.code === "forbidden") ||
                        (failure.status === 422 &&
                          failure.code === "invalid_request"))
                    )
                      recordReplacementRejection(pendingSchedule.key, intent, {
                        status: failure.status,
                        code: failure.code,
                      });
                    throw failure;
                  }
                },
              );
            },
            signal,
            session,
          ),
        "Queued content replaced; the original remains in history",
      );
      if (!signal.aborted && isCurrentWorkspaceSession(session)) {
        if (done) onClose();
        else {
          await onRefresh();
          if (signal.aborted || !isCurrentWorkspaceSession(session)) return;
          setReview(false);
          setMessage(
            "Replacement was not confirmed. Refresh and review the original before another attempt.",
          );
        }
      }
    } finally {
      running.current = false;
      if (!signal.aborted && isCurrentWorkspaceSession(session)) setBusy(false);
    }
  };
  return (
    <section className="detail-section form-stack">
      <h3>Edit queued content</h3>
      <p>
        The current draft is revision {post.version}; this queued snapshot is
        revision {publication.post_version}. Saving a draft does not change the
        queue.
      </p>
      <div className="publication-actions">
        <Button
          variant="secondary"
          disabled={pending || busy}
          onClick={() => {
            onClose();
            onEdit(post);
          }}
        >
          Edit saved draft
        </Button>
        <Button
          variant="secondary"
          disabled={pending || busy}
          onClick={() => setReview(!review)}
        >
          {review ? "Hide comparison" : "Compare and replace"}
        </Button>
      </div>
      {message && <ErrorNotice message={message} />}
      {review && (
        <>
          <div className="form-stack">
            <h4>Queued snapshot</h4>
            <strong>{publication.payload.title || "Untitled"}</strong>
            <p className="post-copy">{publication.payload.text}</p>
            <p>
              Media:{" "}
              {publication.payload.media_ids
                .map((id) => media.find((item) => item.id === id)?.name ?? id)
                .join(", ") || "None"}
            </p>
            {publication.payload.media_ids.map((id) => {
              const frozen = publication.payload.media_metadata?.find(
                (row) => row.id === id,
              );
              const name = media.find((item) => item.id === id)?.name ?? id;
              return (
                <p key={id}>
                  {name} snapshot description:{" "}
                  {frozen
                    ? frozen.alt_text || "None"
                    : "Unavailable in this snapshot"}
                </p>
              );
            })}
            <p>
              Options: <code>{JSON.stringify(oldOptions)}</code>
            </p>
            <h4>Current saved draft</h4>
            <strong>{post.title || "Untitled"}</strong>
            <p className="post-copy">{newText}</p>
            <p>
              Media:{" "}
              {newMedia
                .map((id) => media.find((item) => item.id === id)?.name ?? id)
                .join(", ") || "None"}
            </p>
            {newMedia.map((id) => {
              const item = media.find((row) => row.id === id);
              return item ? (
                <p key={id}>
                  {item.name} description: {item.alt_text || "None"}
                </p>
              ) : null;
            })}
            <p>
              Options: <code>{JSON.stringify(newOptions)}</code>
            </p>
          </div>
          <p className="muted">
            The old snapshot will be canceled and retained in history. The
            replacement keeps the same scheduled time.
          </p>
          <Button
            disabled={
              pending ||
              busy ||
              !pendingSchedule.ready ||
              !!pendingSchedule.intent ||
              !pendingMedia.ready ||
              !publication.scheduling_mode
            }
            onClick={() => void replace()}
          >
            Replace queued content
          </Button>
        </>
      )}
    </section>
  );
}
