import { ArrowUpRight, ShieldCheck } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { z } from "zod";
import { api, authLock, rotateAuthEpoch } from "./api";
import { Unicorn } from "./components/brand";
import { Busy, ErrorNotice, Field, Submit } from "./components/shared";
export function Auth({
  onReady,
  signingOut,
  signOutError,
}: {
  onReady: (epoch: string) => void;
  signingOut: boolean;
  signOutError: string;
}) {
  const [initialized, setInitialized] = useState<boolean | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const activeRequest = useRef<AbortController | null>(null);
  useEffect(() => () => activeRequest.current?.abort(), []);
  useEffect(() => {
    api("/auth/status", z.object({ initialized: z.boolean() }))
      .then((d) => setInitialized(d.initialized))
      .catch((e) => setError(e.message));
  }, []);
  const submit = async (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    if (signingOut) return;
    setBusy(true);
    setError("");
    const data = new FormData(e.currentTarget);
    const controller = new AbortController();
    activeRequest.current = controller;
    try {
      const epoch = await authLock(async () => {
        await api(
          initialized ? "/auth/login" : "/auth/bootstrap",
          z.object({ id: z.string(), email: z.string() }),
          "POST",
          Object.fromEntries(data),
          controller.signal,
        );
        return rotateAuthEpoch();
      }, controller.signal);
      onReady(epoch);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      if (activeRequest.current === controller) activeRequest.current = null;
      setBusy(false);
    }
  };
  return (
    <main className="auth-shell">
      <section className="auth-story">
        <div className="wordmark">
          <Unicorn size={40} />
          beweft<span className="wordmark-dot">✦</span>
        </div>
        <div>
          <div className="eyebrow">A LITTLE MAGIC. ALL YOURS.</div>
          <h1>
            Your ideas,
            <br />
            beautifully
            <br />
            <em>in motion.</em>
          </h1>
          <p>
            A thoughtful home for your brands, your creativity, and the
            conversations that follow.
          </p>
        </div>
        <div className="auth-trail">
          <span>Imagine</span>
          <i />
          <span>Create</span>
          <i />
          <span>Connect</span>
        </div>
        <Unicorn size={260} className="auth-unicorn" />
      </section>
      <section className="auth-form">
        <div className="mobile-wordmark">
          <Unicorn />
          beweft
        </div>
        {initialized === null ? (
          error || signOutError ? (
            <ErrorNotice message={error || signOutError} />
          ) : (
            <Busy />
          )
        ) : (
          <>
            <div className="eyebrow">YOUR OWN CORNER OF THE INTERNET</div>
            <h2>{initialized ? "Welcome back" : "Welcome to beweft"}</h2>
            <p className="muted">
              {initialized
                ? "Pick up where your inspiration left off."
                : "Create your owner account to start your workspace."}
            </p>
            <form onSubmit={submit} className="form-stack">
              {!initialized && (
                <Field
                  label="Setup secret"
                  hint="Find BEWEFT_SETUP_SECRET in your server configuration."
                >
                  <input
                    name="setup_secret"
                    type="password"
                    required
                    autoComplete="off"
                  />
                </Field>
              )}
              <Field label="Email address">
                <input
                  name="email"
                  type="email"
                  required
                  autoComplete="username"
                  placeholder="you@example.com"
                />
              </Field>
              <Field
                label="Password"
                hint={!initialized ? "Use at least 12 characters." : undefined}
              >
                <input
                  name="password"
                  type="password"
                  required
                  minLength={initialized ? undefined : 12}
                  autoComplete={
                    initialized ? "current-password" : "new-password"
                  }
                />
              </Field>
              {(error || signOutError) && (
                <ErrorNotice message={error || signOutError} />
              )}
              <Submit pending={busy || signingOut}>
                {signingOut
                  ? "Signing out…"
                  : initialized
                    ? "Sign in"
                    : "Create your workspace"}
                <ArrowUpRight size={17} />
              </Submit>
            </form>
            <p className="auth-footnote">
              <ShieldCheck size={16} />
              Self-hosted. Your accounts. Your creative space.
            </p>
          </>
        )}
      </section>
    </main>
  );
}
