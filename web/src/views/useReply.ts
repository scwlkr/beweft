import { useEffect, useRef, useState } from "react";
import { z } from "zod";
import { api, ApiError } from "../api";
import {
  readIntent,
  forgetIntent,
  lockIntent,
  type Pending,
} from "./replyIntent";
import { ItemSchema } from "./insight-contracts";
const Attempt = z.object({
  id: z.string().uuid(),
  inbox_id: z.string(),
  text: z.string(),
  state: z.enum(["sending", "uncertain", "published", "failed"]),
  error: z.string().nullable().optional(),
  receipt: z.unknown().optional(),
  created_at: z.string(),
  updated_at: z.string(),
});
type Attempt = z.infer<typeof Attempt>;
const unresolved = (attempt: Attempt) =>
  ["sending", "uncertain"].includes(attempt.state);

// Browser storage remembers identity during a lost response; only server rows establish outcomes.
export function useReply(brandId: string, item: z.infer<typeof ItemSchema>) {
  const [text, setText] = useState("");
  const [attempt, setAttempt] = useState<Attempt | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [historyChanged, setHistoryChanged] = useState(false);
  const [latest, setLatest] = useState<Attempt | null>(null);
  const lastSeen = useRef<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState("");
  const key = useRef("");
  const alive = useRef(true);
  const running = useRef<object | null>(null);
  const recoveryController = useRef<AbortController | null>(null);
  const identity = `${brandId}:${item.channel_id}:${item.id}`;
  const currentIdentity = useRef(identity);
  currentIdentity.current = identity;
  const current = () => alive.current && currentIdentity.current === identity;

  const recover = async () => {
    if (!current()) return null;
    recoveryController.current?.abort();
    const controller = new AbortController();
    recoveryController.current = controller;
    const active = () =>
      current() &&
      !controller.signal.aborted &&
      recoveryController.current === controller;
    setReady(false);
    try {
      const owner = await api(
        "/auth/me",
        z.object({ id: z.string() }),
        "GET",
        undefined,
        controller.signal,
      );
      if (!active()) return null;
      const storageKey = `beweft.pending-reply:${owner.id}:${identity}`;
      const rows = await api(
        `/inbox/${item.id}/replies`,
        z.array(Attempt),
        "GET",
        undefined,
        controller.signal,
      );
      if (rows.some((row) => row.inbox_id !== item.id))
        throw new Error(
          "Reply recovery returned a different conversation. Sending is blocked.",
        );
      const remembered = readIntent(storageKey);
      const ordered = rows;
      const outstanding = ordered.find(unresolved);
      const recorded =
        outstanding ??
        (remembered
          ? (ordered.find((row) => row.id === remembered.request_id) ?? null)
          : (ordered[0] ?? null));
      if (!active()) return null;
      key.current = storageKey;
      lastSeen.current = ordered[0]?.id ?? null;
      setLatest(ordered[0] ?? null);
      setHistoryChanged(
        !!remembered &&
          remembered.expected_last_request_id !== lastSeen.current &&
          !ordered.some((row) => row.id === remembered.request_id),
      );
      setAttempt(recorded);
      setPending(remembered);
      if (recorded || remembered) setText(recorded?.text ?? remembered!.text);
      if (
        recorded &&
        !unresolved(recorded) &&
        recorded.id === remembered?.request_id
      ) {
        await lockIntent(storageKey, () => {
          if (active()) forgetIntent(storageKey, recorded.id);
        });
        if (!active()) return null;
        setPending(null);
      }
      setReady(true);
      setError(recorded?.error ?? "");
      return { rows, recorded, remembered };
    } finally {
      if (recoveryController.current === controller)
        recoveryController.current = null;
    }
  };
  const check = async () => {
    if (running.current) return;
    const operation = {};
    running.current = operation;
    setBusy(true);
    setError("");
    try {
      await recover();
    } catch (failure) {
      if (current() && running.current === operation)
        setError(
          `${(failure as Error).message} The existing reply must be checked before sending.`,
        );
    } finally {
      if (running.current === operation) {
        running.current = null;
        if (current()) setBusy(false);
      }
    }
  };
  useEffect(() => {
    alive.current = true;
    void check();
    return () => {
      alive.current = false;
      recoveryController.current?.abort();
      running.current = null;
    };
    // The parent keys this hook's dialog by brand, channel, and inbox item.
  }, [identity]);

  const send = async () => {
    if (running.current || !ready || (attempt && unresolved(attempt))) return;
    const operation = {};
    running.current = operation;
    setBusy(true);
    setError("");
    const intentAtClick = pending;
    const textAtClick = text;
    const headAtClick = lastSeen.current;
    try {
      // Recheck durable attempts immediately before sending. The server also guards
      // concurrent tabs, which can race between this read and the POST.
      const recovered = await recover();
      if (!recovered || !current() || running.current !== operation) return;
      if (recovered.rows.some(unresolved)) return;
      const head = recovered.rows[0]?.id ?? null;
      // Claim only the intent the user actually saw. Another tab may have written
      // an intent while this tab's GET was in flight; do not overwrite or send it.
      const outgoing = await lockIntent(key.current, () => {
        if (!current() || running.current !== operation) return null;
        const remembered = readIntent(key.current) ?? recovered.remembered;
        if (
          remembered &&
          (!intentAtClick ||
            remembered.request_id !== intentAtClick.request_id ||
            remembered.text !== intentAtClick.text ||
            remembered.expected_last_request_id !==
              intentAtClick.expected_last_request_id)
        ) {
          setPending(remembered);
          setAttempt(null);
          setText(remembered.text);
          setHistoryChanged(remembered.expected_last_request_id !== head);
          setError(
            "Another tab has an existing reply intent. Review it, then explicitly retry the original reply if needed.",
          );
          return null;
        }
        const candidate = remembered ?? intentAtClick;
        const prior =
          candidate &&
          recovered.rows.find((row) => row.id === candidate.request_id);
        if (prior) {
          setAttempt(prior);
          setText(prior.text);
          return null;
        }
        if (
          head !==
          (candidate ? candidate.expected_last_request_id : headAtClick)
        ) {
          setHistoryChanged(!!candidate);
          setError(
            "Reply history changed. Review the latest server outcome before confirming another send.",
          );
          return null;
        }
        const claimed = candidate ?? {
          request_id: crypto.randomUUID(),
          text: textAtClick,
          expected_last_request_id: headAtClick,
        };
        localStorage.setItem(key.current, JSON.stringify(claimed));
        return claimed;
      });
      if (!outgoing || !current() || running.current !== operation) return;
      setPending(outgoing);
      setText(outgoing.text);
      setAttempt(null);
      const result = await api(
        `/inbox/${item.id}/reply`,
        Attempt,
        "POST",
        outgoing,
      );
      if (result.id !== outgoing.request_id || result.inbox_id !== item.id)
        throw new Error(
          "Reply response identity did not match. Check the existing attempt.",
        );
      if (!current() || running.current !== operation) return;
      setAttempt(result);
      setError(result.error ?? "");
      if (!unresolved(result)) {
        await lockIntent(key.current, () =>
          forgetIntent(key.current, result.id),
        );
        setPending(null);
      }
    } catch (failure) {
      if (current() && running.current === operation) {
        setReady(false);
        setError(
          failure instanceof ApiError &&
            failure.code === "reply_history_changed"
            ? "Another reply changed the server history. Check the existing reply outcome and explicitly review it before retrying this intent."
            : `${(failure as Error).message} The reply outcome is unconfirmed. Check the existing attempt before sending again.`,
        );
      }
    } finally {
      if (running.current === operation) {
        running.current = null;
        if (current()) setBusy(false);
      }
    }
  };
  const acknowledgeHistory = async () => {
    if (!pending || !ready || busy || (latest && unresolved(latest))) return;
    try {
      const updated = await lockIntent(key.current, () => {
        const stored = readIntent(key.current);
        if (stored?.request_id !== pending.request_id) return null;
        const acknowledged = {
          ...stored,
          expected_last_request_id: latest?.id ?? null,
        };
        localStorage.setItem(key.current, JSON.stringify(acknowledged));
        return acknowledged;
      });
      if (updated && current()) {
        setPending(updated);
        setHistoryChanged(false);
        setError(
          "History acknowledged. Retry the original reply explicitly only if it is still needed.",
        );
      } else await check();
    } catch (failure) {
      setReady(false);
      setError((failure as Error).message);
    }
  };
  const resolve = async (body: {
    resolution: "published" | "not_published";
    note: string;
    external_url?: string;
  }) => {
    if (!attempt || attempt.state !== "uncertain" || running.current || !ready)
      return;
    const operation = {};
    running.current = operation;
    setBusy(true);
    setError("");
    try {
      const result = await api(
        `/inbox/${item.id}/replies/${attempt.id}/resolve`,
        Attempt,
        "POST",
        body,
      );
      if (result.id !== attempt.id || result.inbox_id !== item.id)
        throw new Error("Reply resolution identity did not match.");
      if (!current() || running.current !== operation) return;
      await recover();
    } catch (failure) {
      if (current() && running.current === operation) {
        setReady(false);
        setError(
          `${(failure as Error).message} Check the recorded outcome before continuing.`,
        );
      }
    } finally {
      if (running.current === operation) {
        running.current = null;
        if (current()) setBusy(false);
      }
    }
  };
  const another = () => {
    if (!ready || !attempt || unresolved(attempt) || busy) return;
    if (pending && pending.request_id !== attempt.id) return;
    setAttempt(null);
    setPending(null);
    setText("");
    setError("");
  };
  return {
    text,
    setText,
    attempt,
    pending,
    busy,
    ready,
    error,
    check,
    send,
    another,
    resolve,
    historyChanged,
    latest,
    acknowledgeHistory,
    locked: !ready || busy || !!pending || !!attempt,
  };
}
