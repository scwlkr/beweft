import { useCallback, useEffect, useRef, useState } from "react";
import { z } from "zod";
import {
  api,
  captureWorkspaceSession,
  isCurrentWorkspaceSession,
  ok,
} from "../api";
import { ErrorNotice, Field, Submit, Time } from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";
const Session = z.object({
  id: z.string(),
  created_at: z.string(),
  expires_at: z.string(),
  current: z.boolean(),
});
type Session = z.infer<typeof Session>;
type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;
export function Security({
  action,
  pending,
  endSessionWith,
}: {
  action: Action;
  pending: boolean;
  endSessionWith: (
    path: string,
    method: "POST" | "DELETE",
    body?: unknown,
  ) => Promise<void>;
}) {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [error, setError] = useState("");
  const [currentPassword, setCurrentPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState<Session | "others" | null>(null);
  const alive = useRef(false);
  const running = useRef(false);
  const sequence = useRef(0);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      ++sequence.current;
    };
  }, []);
  const load = useCallback(async () => {
    let session;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    const id = ++sequence.current;
    try {
      const rows = await api("/auth/sessions", z.array(Session));
      if (
        alive.current &&
        id === sequence.current &&
        isCurrentWorkspaceSession(session)
      ) {
        setSessions(rows);
        setError("");
      }
    } catch (failure) {
      if (
        alive.current &&
        id === sequence.current &&
        isCurrentWorkspaceSession(session)
      )
        setError((failure as Error).message);
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load]);
  const changePassword = async () => {
    if (running.current || pending) return;
    if (newPassword !== confirmPassword) {
      setError("The new passwords do not match.");
      return;
    }
    running.current = true;
    setBusy(true);
    setError("");
    try {
      await endSessionWith("/auth/password", "POST", {
        current_password: currentPassword,
        new_password: newPassword,
      });
    } catch (failure) {
      if (alive.current) setError((failure as Error).message);
    } finally {
      running.current = false;
      if (alive.current) {
        setBusy(false);
        setCurrentPassword("");
        setNewPassword("");
        setConfirmPassword("");
      }
    }
  };
  const revoke = async () => {
    if (!confirm || running.current || pending) return;
    if (confirm !== "others" && confirm.current) {
      running.current = true;
      setBusy(true);
      try {
        await endSessionWith(`/auth/sessions/${confirm.id}`, "DELETE");
      } catch (failure) {
        if (alive.current) setError((failure as Error).message);
      } finally {
        running.current = false;
        if (alive.current) setBusy(false);
      }
      return;
    }
    const done = await action(
      () =>
        confirm === "others"
          ? api("/auth/sessions/revoke-others", ok, "POST", {})
          : api(`/auth/sessions/${confirm.id}`, ok, "DELETE"),
      "Sessions updated",
    );
    if (alive.current) {
      if (done) setConfirm(null);
      await load();
    }
  };
  return (
    <section className="settings-section form-stack">
      <h2>Owner security</h2>
      <p className="muted">
        Manage browser sessions and change the owner password. API tokens are
        managed separately.
      </p>
      {error && <ErrorNotice message={error} />}
      <div className="publication-actions">
        <Button variant="secondary" onClick={() => void load()}>
          Refresh sessions
        </Button>
        <Button
          variant="secondary"
          disabled={pending || busy}
          onClick={() => setConfirm("others")}
        >
          Revoke other sessions
        </Button>
      </div>
      {sessions.map((session) => (
        <div className="token-row" key={session.id}>
          <div>
            <strong>
              {session.current ? "This session" : "Other session"}
            </strong>
            <small>
              Created <Time value={session.created_at} /> · expires{" "}
              <Time value={session.expires_at} />
            </small>
          </div>
          <Button
            variant="ghost"
            size="sm"
            disabled={pending || busy}
            onClick={() => setConfirm(session)}
          >
            Revoke
          </Button>
        </div>
      ))}
      <form
        className="form-stack"
        onSubmit={(event) => {
          event.preventDefault();
          void changePassword();
        }}
      >
        <h3>Change password</h3>
        <p className="muted">
          Changing your password signs out every owner session.
        </p>
        <Field label="Current password">
          <input
            type="password"
            autoComplete="current-password"
            required
            value={currentPassword}
            onChange={(event) => setCurrentPassword(event.target.value)}
          />
        </Field>
        <Field label="New password" hint="At least 12 characters.">
          <input
            type="password"
            autoComplete="new-password"
            required
            minLength={12}
            value={newPassword}
            onChange={(event) => setNewPassword(event.target.value)}
          />
        </Field>
        <Field label="Confirm new password">
          <input
            type="password"
            autoComplete="new-password"
            required
            minLength={12}
            value={confirmPassword}
            onChange={(event) => setConfirmPassword(event.target.value)}
          />
        </Field>
        <Submit pending={busy || pending}>Change password and sign out</Submit>
      </form>
      {confirm && (
        <Dialog
          open
          onOpenChange={(open) => !open && setConfirm(null)}
          title={
            confirm === "others"
              ? "Revoke other sessions?"
              : confirm.current
                ? "Revoke this session?"
                : "Revoke session?"
          }
          description={
            confirm !== "others" && confirm.current
              ? "You will return to sign-in."
              : "The affected sessions will need to sign in again."
          }
        >
          <div className="dialog-footer">
            <Button variant="secondary" onClick={() => setConfirm(null)}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              disabled={busy || pending}
              onClick={() => void revoke()}
            >
              Revoke
            </Button>
          </div>
        </Dialog>
      )}
    </section>
  );
}
