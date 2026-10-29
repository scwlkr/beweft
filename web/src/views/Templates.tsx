import { useCallback, useEffect, useRef, useState } from "react";
import { z } from "zod";
import {
  api,
  captureWorkspaceSession,
  isCurrentWorkspaceSession,
  ok,
  PostSchema,
  VariantSchema,
  type Brand,
  type Post,
  type Channel,
} from "../api";
import { ErrorNotice, Field, Submit, Time } from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";

const Starter = z.object({
  id: z.string(),
  name: z.string(),
  title: z.string(),
  text: z.string(),
});
const Template = z.object({
  id: z.string(),
  brand_id: z.string(),
  name: z.string(),
  title: z.string(),
  text: z.string(),
  media_ids: z.array(z.string()),
  variants: z.array(VariantSchema),
  options: z.record(z.string(), z.unknown()),
  version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});
type Template = z.infer<typeof Template>;
type Starter = z.infer<typeof Starter>;
type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;
type Editor = {
  id?: string;
  version?: number;
  name: string;
  title: string;
  text: string;
  media_ids: string[];
  variants: Post["variants"];
  options: Record<string, unknown>;
};
const blank = (): Editor => ({
  name: "",
  title: "",
  text: "",
  media_ids: [],
  variants: [],
  options: {},
});

export function Templates({
  brand,
  posts,
  channels,
  action,
  pending,
  onEdit,
}: {
  brand: Brand;
  posts: Post[];
  channels: Channel[];
  action: Action;
  pending: boolean;
  onEdit: (post: Post) => void;
}) {
  const [templates, setTemplates] = useState<Template[]>([]);
  const [starters, setStarters] = useState<Starter[]>([]);
  const [editor, setEditor] = useState<Editor | null>(null);
  const [applying, setApplying] = useState<Template | null>(null);
  const [deleting, setDeleting] = useState<Template | null>(null);
  const [values, setValues] = useState<Record<string, string>>({});
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const alive = useRef(false);
  const sequence = useRef(0);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      ++sequence.current;
    };
  }, []);
  const refresh = useCallback(async () => {
    let session;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    const current = ++sequence.current;
    setLoading(true);
    try {
      const [saved, original] = await Promise.all([
        api(
          `/templates?brand_id=${encodeURIComponent(brand.id)}`,
          z.array(Template),
        ),
        api("/template-starters", z.array(Starter)),
      ]);
      if (
        !alive.current ||
        current !== sequence.current ||
        !isCurrentWorkspaceSession(session)
      )
        return;
      setTemplates(saved);
      setStarters(original);
      setError("");
    } catch (failure) {
      if (
        alive.current &&
        current === sequence.current &&
        isCurrentWorkspaceSession(session)
      )
        setError((failure as Error).message);
    } finally {
      if (
        alive.current &&
        current === sequence.current &&
        isCurrentWorkspaceSession(session)
      )
        setLoading(false);
    }
  }, [brand.id]);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  const save = async () => {
    if (!editor || !editor.name.trim()) return;
    const { id, version, ...content } = editor;
    const saved = await action(
      () =>
        api(
          id ? `/templates/${id}` : "/templates",
          Template,
          id ? "PATCH" : "POST",
          id ? { ...content, version } : { ...content, brand_id: brand.id },
        ),
      id ? "Template updated" : "Template saved",
    );
    if (!alive.current) return;
    if (saved) setEditor(null);
    await refresh();
  };
  const apply = async () => {
    if (!applying) return;
    let post: Post | undefined;
    const variables = Object.fromEntries(
      Object.entries(values).filter(([, value]) => value !== ""),
    );
    const saved = await action(async () => {
      post = await api(`/templates/${applying.id}/apply`, PostSchema, "POST", {
        variables,
      });
    }, "Draft created from template");
    if (saved && alive.current && post) {
      setApplying(null);
      onEdit(post);
    }
  };
  const keys = applying
    ? [
        ...new Set(
          [
            applying.title,
            applying.text,
            ...applying.variants.map((v) => v.text ?? ""),
          ]
            .flatMap((text) =>
              [...text.matchAll(/\{\{([^{}]+)\}\}/g)].map((match) =>
                match[1].trim(),
              ),
            )
            .filter(Boolean),
        ),
      ]
    : [];
  return (
    <div className="form-stack">
      <div className="view-toolbar">
        <div>
          <h2>Reusable templates</h2>
          <p className="muted">
            Save a starting point, then make an unscheduled draft whenever you
            need it.
          </p>
        </div>
        <Button onClick={() => setEditor(blank())}>New template</Button>
      </div>
      {error && (
        <>
          <ErrorNotice message={error} />
          <Button variant="secondary" onClick={() => void refresh()}>
            Reload templates
          </Button>
        </>
      )}
      {loading && <p role="status">Loading templates…</p>}
      {templates.map((item) => (
        <article className="settings-section form-stack" key={item.id}>
          <div>
            <h3>{item.name}</h3>
            <p className="post-copy">{item.text}</p>
            <small>
              Updated <Time value={item.updated_at} /> · {item.media_ids.length}{" "}
              shared media · {item.variants.length} channel variants
            </small>
          </div>
          <div className="publication-actions">
            <Button
              onClick={() => {
                setApplying(item);
                setValues({});
              }}
            >
              Use template
            </Button>
            <Button
              variant="secondary"
              onClick={() =>
                setEditor({
                  id: item.id,
                  version: item.version,
                  name: item.name,
                  title: item.title,
                  text: item.text,
                  media_ids: item.media_ids,
                  variants: item.variants,
                  options: item.options,
                })
              }
            >
              Edit
            </Button>
            <Button variant="ghost" onClick={() => setDeleting(item)}>
              Delete
            </Button>
          </div>
        </article>
      ))}
      {!loading && templates.length === 0 && !error && (
        <p className="muted">
          No saved templates yet. Start with a blank template, a starter, or a
          draft.
        </p>
      )}
      {starters.length > 0 && (
        <section className="settings-section">
          <h3>Starter ideas</h3>
          <p className="muted">
            These are starting points; save one to customize it.
          </p>
          <div className="publication-actions">
            {starters.map((starter) => (
              <Button
                variant="secondary"
                key={starter.id}
                onClick={() =>
                  setEditor({ ...blank(), ...starter, id: undefined })
                }
              >
                {starter.name}
              </Button>
            ))}
          </div>
        </section>
      )}
      {editor && (
        <Dialog
          open
          onOpenChange={(open) => !open && setEditor(null)}
          title={editor.id ? "Edit template" : "New template"}
          description="Use {{variable}} in titles or text. Media, channel variants, and options from a draft are retained."
        >
          <form
            className="form-stack"
            onSubmit={(event) => {
              event.preventDefault();
              void save();
            }}
          >
            {posts.some((post) => post.status === "draft") && (
              <Field
                label={
                  editor.id
                    ? "Replace contents from a draft"
                    : "Start from a draft"
                }
              >
                <select
                  defaultValue=""
                  onChange={(event) => {
                    const post = posts.find(
                      (row) => row.id === event.target.value,
                    );
                    if (post)
                      setEditor({
                        ...editor,
                        title: post.title,
                        text: post.text,
                        media_ids: post.media_ids,
                        variants: post.variants,
                        options: post.options ?? {},
                      });
                  }}
                >
                  <option value="">Blank or starter</option>
                  {posts
                    .filter((post) => post.status === "draft")
                    .map((post) => (
                      <option key={post.id} value={post.id}>
                        {post.title ||
                          post.text.slice(0, 60) ||
                          "Untitled draft"}
                      </option>
                    ))}
                </select>
              </Field>
            )}
            <Field label="Template name">
              <input
                required
                maxLength={120}
                value={editor.name}
                onChange={(event) =>
                  setEditor({ ...editor, name: event.target.value })
                }
              />
            </Field>
            <Field label="Post title">
              <input
                value={editor.title}
                onChange={(event) =>
                  setEditor({ ...editor, title: event.target.value })
                }
              />
            </Field>
            <Field label="Post text">
              <textarea
                rows={7}
                value={editor.text}
                onChange={(event) =>
                  setEditor({ ...editor, text: event.target.value })
                }
              />
            </Field>
            {editor.variants.map((variant, index) => (
              <Field
                key={variant.channel_id}
                label={`Channel text · ${channels.find((channel) => channel.id === variant.channel_id)?.name ?? variant.channel_id}`}
                hint="Leave unchanged to retain the saved channel text. Media and options remain attached."
              >
                <textarea
                  rows={3}
                  value={variant.text ?? ""}
                  placeholder={
                    variant.text === undefined ? "Uses shared text" : ""
                  }
                  onChange={(event) =>
                    setEditor({
                      ...editor,
                      variants: editor.variants.map((row, current) =>
                        current === index
                          ? { ...row, text: event.target.value }
                          : row,
                      ),
                    })
                  }
                />
              </Field>
            ))}
            <p className="muted">
              {editor.media_ids.length} shared media and{" "}
              {editor.variants.length} channel variants retained. To change
              them, choose an updated draft above.
            </p>
            <Submit pending={pending}>Save template</Submit>
          </form>
        </Dialog>
      )}
      {applying && (
        <Dialog
          open
          onOpenChange={(open) => !open && setApplying(null)}
          title={`Use ${applying.name}`}
          description="A new draft will be created, without scheduling. Leave a value blank to keep its placeholder editable; brand defaults to this brand’s name."
        >
          <form
            className="form-stack"
            onSubmit={(event) => {
              event.preventDefault();
              void apply();
            }}
          >
            {keys.map((key) => (
              <Field key={key} label={key}>
                <input
                  value={values[key] ?? ""}
                  placeholder={key === "brand" ? brand.name : `{{${key}}}`}
                  onChange={(event) =>
                    setValues({ ...values, [key]: event.target.value })
                  }
                />
              </Field>
            ))}
            <Submit pending={pending}>Create unscheduled draft</Submit>
          </form>
        </Dialog>
      )}
      {deleting && (
        <Dialog
          open
          onOpenChange={(open) => !open && setDeleting(null)}
          title="Delete template?"
          description={`Delete ${deleting.name}? Existing drafts are retained.`}
        >
          <div className="dialog-footer">
            <Button variant="secondary" onClick={() => setDeleting(null)}>
              Keep template
            </Button>
            <Button
              variant="destructive"
              disabled={pending}
              onClick={() =>
                void action(
                  () => api(`/templates/${deleting.id}`, ok, "DELETE"),
                  "Template deleted",
                ).then(async (done) => {
                  if (!alive.current) return;
                  if (done) setDeleting(null);
                  await refresh();
                })
              }
            >
              Delete template
            </Button>
          </div>
        </Dialog>
      )}
    </div>
  );
}
