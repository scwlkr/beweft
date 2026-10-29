import { ArrowUpRight, Copy, ShieldCheck } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { z } from "zod";
import { api, ok } from "../api";
import { ErrorNotice, Field, Submit } from "../components/shared";
import { Button } from "../components/ui/button";

type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;
const TokenSchema = z
  .object({
    id: z.string(),
    name: z.string(),
    scopes: z.array(z.string()),
    brand_ids: z.array(z.string()).nullable().optional(),
    created_at: z.string().optional(),
  })
  .passthrough();
export function Tokens({
  action,
  pending,
}: {
  action: Action;
  pending: boolean;
}) {
  const [tokens, setTokens] = useState<z.infer<typeof TokenSchema>[]>([]);
  const [name, setName] = useState("");
  const [scopes, setScopes] = useState(["read"]);
  const [secret, setSecret] = useState("");
  const [error, setError] = useState("");
  const load = useCallback(
    () =>
      api("/tokens", z.array(TokenSchema))
        .then(setTokens)
        .catch((e) => setError(e.message)),
    [],
  );
  useEffect(() => {
    void load();
  }, [load]);
  return (
    <section className="settings-section">
      <h2>Give your tools the right access</h2>
      <p className="muted">
        API tokens use the same authoritative actions as this app. Grant only
        what each integration needs; never put a token into client-side code.
      </p>
      <a
        href="/api-docs/openapi.json"
        className="text-link"
        target="_blank"
        rel="noreferrer"
      >
        OpenAPI contract <ArrowUpRight size={15} />
      </a>
      {error && <ErrorNotice message={error} />}
      <div className="token-list">
        {tokens.map((t) => (
          <div className="token-row" key={t.id}>
            <ShieldCheck size={19} />
            <div>
              <strong>{t.name}</strong>
              <small>{t.scopes.join(" · ")}</small>
            </div>
            <Button
              variant="ghost"
              size="sm"
              disabled={pending}
              onClick={() =>
                void action(
                  () => api(`/tokens/${t.id}`, ok, "DELETE"),
                  "Token revoked",
                ).then(async (done) => {
                  if (done) await load();
                })
              }
            >
              Revoke
            </Button>
          </div>
        ))}
      </div>
      {secret && (
        <div className="secret-notice" role="status">
          <strong>Copy your token now</strong>
          <p>It will not be shown again. Store it in your password manager.</p>
          <code>{secret}</code>
          <div className="inline-actions">
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void navigator.clipboard.writeText(secret)}
            >
              <Copy size={15} />
              Copy token
            </Button>
            <Button variant="ghost" size="sm" onClick={() => setSecret("")}>
              I saved it
            </Button>
          </div>
        </div>
      )}
      <form
        className="form-stack"
        onSubmit={(e) => {
          e.preventDefault();
          void action(async () => {
            const result = await api(
              "/tokens",
              TokenSchema.extend({ token: z.string() }),
              "POST",
              { name, scopes },
            );
            setSecret(result.token);
            setName("");
          }, "API token created").then(async (done) => {
            if (done) await load();
          });
        }}
      >
        <h3>Create a token</h3>
        <Field label="Token name">
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
            placeholder="My writing assistant"
            maxLength={100}
          />
        </Field>
        <fieldset className="scope-options">
          <legend>Permissions</legend>
          {[
            {
              id: "read",
              name: "Read workspace",
              help: "Inspect posts, channels, and outcomes.",
            },
            {
              id: "drafts:write",
              name: "Create and edit drafts",
              help: "Manage ideas, drafts, and media.",
            },
            {
              id: "publishing:write",
              name: "Manage publishing",
              help: "Schedule posts and resolve publishing outcomes.",
            },
            {
              id: "admin",
              name: "Administration",
              help: "Manage brands and channel configuration.",
            },
          ].map((s) => (
            <label key={s.id}>
              <input
                type="checkbox"
                checked={scopes.includes(s.id)}
                onChange={(e) =>
                  setScopes(
                    e.target.checked
                      ? [...scopes, s.id]
                      : scopes.filter((v) => v !== s.id),
                  )
                }
              />
              <span>
                <strong>{s.name}</strong>
                <small>{s.help}</small>
              </span>
            </label>
          ))}
        </fieldset>
        <Submit pending={pending}>Create token</Submit>
      </form>
    </section>
  );
}
