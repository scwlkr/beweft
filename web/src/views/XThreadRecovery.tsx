import { useEffect, useRef, useState } from "react";
import {
  api,
  captureWorkspaceSession,
  isCurrentWorkspaceSession,
  PublicationSchema,
} from "../api";
import type {
  ReconcileRequest,
  XThreadStatus,
} from "../lib/api-contract.generated";
import { Field } from "../components/shared";
import { Button } from "../components/ui/button";

type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;

export function XThreadRecovery({
  publicationId,
  thread,
  action,
  pending,
  onChanged,
}: {
  publicationId: string;
  thread: XThreadStatus;
  action: Action;
  pending: boolean;
  onChanged: () => void;
}) {
  const [ids, setIds] = useState(() => [
    ...thread.confirmed_ids,
    ...Array.from(
      { length: thread.total - thread.confirmed_ids.length },
      () => "",
    ),
  ]);
  const [note, setNote] = useState("");
  const [stopped, setStopped] = useState(false);
  const [inspected, setInspected] = useState(false);
  const [rootEnded, setRootEnded] = useState(false);
  const [busy, setBusy] = useState(false);
  const alive = useRef(false);
  const running = useRef(false);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const validIds =
    ids.length === thread.total &&
    ids.every((id) => /^\d{1,19}$/.test(id)) &&
    new Set(ids).size === ids.length;
  const submit = async (resolution: "published" | "not_published") => {
    if (
      running.current ||
      pending ||
      !note.trim() ||
      !stopped ||
      (resolution === "published" ? !inspected || !validIds : !rootEnded)
    )
      return;
    let session;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    const body: ReconcileRequest = {
      resolution,
      note,
      observed_attempt_id: thread.attempt_id,
      observed_checkpoint_id: thread.checkpoint_id,
      sender_stopped: true,
      provider_inspected: true,
      ...(resolution === "published"
        ? { thread_ids: ids }
        : { thread_root_definitively_ended: true }),
    };
    running.current = true;
    setBusy(true);
    try {
      await action(
        () =>
          api(
            `/publications/${publicationId}/reconcile`,
            PublicationSchema,
            "POST",
            body,
          ),
        "Owner observation recorded",
      );
      if (alive.current && isCurrentWorkspaceSession(session)) onChanged();
    } finally {
      running.current = false;
      if (alive.current && isCurrentWorkspaceSession(session)) setBusy(false);
    }
  };
  return (
    <div className="form-stack">
      <p>
        {thread.confirmed_ids.length} of {thread.total} posts have provider
        receipts. Unconfirmed text may already be public: inspect the actual
        reply chain before doing anything. Do not restart the whole thread.
      </p>
      {thread.confirmed_ids.map((id, index) => (
        <p key={id}>
          Confirmed post {index + 1}:{" "}
          <a
            href={`https://x.com/i/status/${id}`}
            target="_blank"
            rel="noreferrer"
          >
            {id}
          </a>
        </p>
      ))}
      {thread.remaining_texts.map((text, index) => (
        <div className="notice" key={thread.next_index + index}>
          <strong>Unconfirmed post {thread.next_index + index + 1}</strong>
          <p className="post-copy">
            {text || "First post uses the saved media."}
          </p>
        </div>
      ))}
      <p>
        Attempt: <code>{thread.attempt_id}</code>
        <br />
        Checkpoint: <code>{thread.checkpoint_id}</code>
      </p>
      <p>
        After inspecting X, finish missing posts manually if needed. Record
        completion only when the entire requested chain exists in order. Your
        observation is not provider API confirmation.
      </p>
      {ids.map((id, index) => (
        <Field
          key={index}
          label={`Post ${index + 1} ID${index === 0 ? " (root)" : ""}`}
        >
          <input
            inputMode="numeric"
            value={id}
            readOnly={index < thread.confirmed_ids.length}
            onChange={(event) =>
              setIds(
                ids.map((value, i) =>
                  i === index ? event.target.value : value,
                ),
              )
            }
          />
        </Field>
      ))}
      <Field label="X verification note">
        <textarea
          rows={2}
          value={note}
          onChange={(event) => setNote(event.target.value)}
        />
      </Field>
      <label className="checkbox-label">
        <input
          type="checkbox"
          checked={stopped}
          onChange={(event) => setStopped(event.target.checked)}
        />
        I confirmed the original sender has stopped.
      </label>
      <label className="checkbox-label">
        <input
          type="checkbox"
          checked={inspected}
          onChange={(event) => setInspected(event.target.checked)}
        />
        I inspected the actual X reply chain and verified every ID matches the
        requested posts in order.
      </label>
      <Button
        type="button"
        variant="secondary"
        disabled={
          busy || pending || !note.trim() || !stopped || !inspected || !validIds
        }
        onClick={() => void submit("published")}
      >
        Record complete thread
      </Button>
      {thread.root_request_started &&
        thread.phase === "request_started" &&
        thread.next_index === 0 &&
        thread.confirmed_ids.length === 0 && (
          <div className="form-stack">
            <p>
              If no root was created, the original remote request must have
              definitively ended without creating one before you record that
              outcome. A missing post or stopped local sender alone is not
              proof.
            </p>
            <label className="checkbox-label">
              <input
                type="checkbox"
                checked={rootEnded}
                onChange={(event) => setRootEnded(event.target.checked)}
              />
              I inspected X and confirmed the original root operation
              definitively ended without creating a post.
            </label>
            <Button
              type="button"
              variant="secondary"
              disabled={
                busy || pending || !note.trim() || !stopped || !rootEnded
              }
              onClick={() => void submit("not_published")}
            >
              Record root not published
            </Button>
          </div>
        )}
    </div>
  );
}
