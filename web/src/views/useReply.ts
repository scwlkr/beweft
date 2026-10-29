import { useEffect, useRef, useState } from "react";
import { z } from "zod";
import { api } from "../api";
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
const Pending = z.object({ request_id: z.string().uuid(), text: z.string() });
type Attempt = z.infer<typeof Attempt>;
type Pending = z.infer<typeof Pending>;
const unresolved = (attempt: Attempt) =>
  ["sending", "uncertain"].includes(attempt.state);

// Browser storage remembers identity during a lost response; only server rows establish outcomes.
export function useReply(brandId: string, item: z.infer<typeof ItemSchema>) {
  const [text, setText] = useState("");
  const [attempt, setAttempt] = useState<Attempt | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [busy, setBusy] = useState(false);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState("");
  const key = useRef("");
  const alive = useRef(true);
  const running = useRef(false);
  const identity = `${brandId}:${item.channel_id}:${item.id}`;
  const currentIdentity = useRef(identity);
  currentIdentity.current = identity;
  const current = () => alive.current && currentIdentity.current === identity;

  const recover = async () => {
    setReady(false);
    const owner = await api("/auth/me", z.object({ id: z.string() }));
    const storageKey = `beweft.pending-reply:${owner.id}:${identity}`;
    const rows = await api(`/inbox/${item.id}/replies`, z.array(Attempt));
    if (rows.some((row) => row.inbox_id !== item.id))
      throw new Error(
        "Reply recovery returned a different conversation. Sending is blocked.",
      );
    const saved = localStorage.getItem(storageKey);
    const remembered = saved ? Pending.parse(JSON.parse(saved)) : null;
    const ordered = [...rows].sort((a, b) =>
      b.created_at.localeCompare(a.created_at),
    );
    const outstanding = ordered.find(unresolved);
    const recorded =
      outstanding ??
      (remembered
        ? (ordered.find((row) => row.id === remembered.request_id) ?? null)
        : (ordered[0] ?? null));
    if (!current()) return null;
    key.current = storageKey;
    setAttempt(recorded);
    setPending(remembered);
    if (recorded || remembered) setText(recorded?.text ?? remembered!.text);
    if (
      recorded &&
      !unresolved(recorded) &&
      recorded.id === remembered?.request_id
    ) {
      localStorage.removeItem(storageKey);
      setPending(null);
    }
    setReady(true);
    setError(recorded?.error ?? "");
    return { rows, recorded, remembered };
  };
  const check = async () => {
    if (running.current) return;
    running.current = true;
    setBusy(true);
    setError("");
    try {
      await recover();
    } catch (failure) {
      if (current())
        setError(
          `${(failure as Error).message} The existing reply must be checked before sending.`,
        );
    } finally {
      running.current = false;
      if (current()) setBusy(false);
    }
  };
  useEffect(() => {
    alive.current = true;
    void check();
    return () => {
      alive.current = false;
    };
    // The parent keys this hook's dialog by brand, channel, and inbox item.
  }, [identity]);

  const send = async () => {
    if (running.current || !ready || (attempt && unresolved(attempt))) return;
    running.current = true;
    setBusy(true);
    setError("");
    const outgoing = pending ?? { request_id: crypto.randomUUID(), text };
    try {
      // Recheck durable attempts immediately before sending. The server also guards
      // concurrent tabs, which can race between this read and the POST.
      const recovered = await recover();
      if (!recovered || !current()) return;
      if (recovered.rows.some(unresolved)) return;
      const prior = recovered.rows.find(
        (row) => row.id === outgoing.request_id,
      );
      if (prior) {
        setAttempt(prior);
        setText(prior.text);
        return;
      }
      localStorage.setItem(key.current, JSON.stringify(outgoing));
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
      if (!current()) return;
      setAttempt(result);
      setError(result.error ?? "");
      if (!unresolved(result)) {
        localStorage.removeItem(key.current);
        setPending(null);
      }
    } catch (failure) {
      if (current()) {
        setReady(false);
        setError(
          `${(failure as Error).message} The reply outcome is unconfirmed. Check the existing attempt before sending again.`,
        );
      }
    } finally {
      running.current = false;
      if (current()) setBusy(false);
    }
  };
  const resolve = async (body: {
    resolution: "published" | "not_published";
    note: string;
    external_url?: string;
  }) => {
    if (!attempt || attempt.state !== "uncertain" || running.current || !ready)
      return;
    running.current = true;
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
      await recover();
    } catch (failure) {
      if (current()) {
        setReady(false);
        setError(
          `${(failure as Error).message} Check the recorded outcome before continuing.`,
        );
      }
    } finally {
      running.current = false;
      if (current()) setBusy(false);
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
    locked: !ready || busy || !!pending || !!attempt,
  };
}
