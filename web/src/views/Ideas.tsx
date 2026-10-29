import type {
  CreateIdeaRequest,
  UpdateIdeaRequest,
} from "../lib/api-contract.generated";
import {
  ArrowUpRight,
  Lightbulb,
  MoreHorizontal,
  Plus,
  Trash2,
} from "lucide-react";
import { useState } from "react";
import { api, IdeaSchema, ok, type Brand, type Idea } from "../api";
import { Empty, Field, Submit } from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";

type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;
export function Ideas({
  brand,
  ideas,
  action,
  pending,
  onCompose,
}: {
  brand: Brand;
  ideas: Idea[];
  action: Action;
  pending: boolean;
  onCompose: (i: Idea) => void;
}) {
  const [edit, setEdit] = useState<Idea | null | undefined>(undefined);
  const [title, setTitle] = useState("");
  const [text, setText] = useState("");
  const [color, setColor] = useState("lavender");
  const open = (i: Idea | null) => {
    setEdit(i);
    setTitle(i?.title ?? "");
    setText(i?.text ?? "");
    setColor(i?.color ?? "lavender");
  };
  return (
    <>
      <div className="section-toolbar">
        <p className="muted">
          {ideas.length} {ideas.length === 1 ? "idea" : "ideas"} to explore
        </p>
        <Button onClick={() => open(null)}>
          <Plus size={17} />
          Capture an idea
        </Button>
      </div>
      {ideas.length === 0 ? (
        <Empty
          title="Every good post starts somewhere"
          action={
            <Button onClick={() => open(null)}>
              <Lightbulb size={17} />
              Capture your first idea
            </Button>
          }
        >
          A passing thought, a campaign theme, a sentence you love. Give it a
          home here.
        </Empty>
      ) : (
        <div className="ideas-grid">
          {ideas.map((i) => (
            <article className={`idea-card idea-${i.color}`} key={i.id}>
              <div className="idea-card-top">
                <Lightbulb size={18} />
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label={`Edit ${i.title}`}
                  onClick={() => open(i)}
                >
                  <MoreHorizontal size={18} />
                </Button>
              </div>
              <button className="idea-content" onClick={() => open(i)}>
                <h2>{i.title}</h2>
                <p>{i.text}</p>
              </button>
              <div className="idea-card-bottom">
                <small>
                  {new Date(i.updated_at).toLocaleDateString(undefined, {
                    month: "short",
                    day: "numeric",
                  })}
                </small>
                <Button variant="ghost" size="sm" onClick={() => onCompose(i)}>
                  Create post <ArrowUpRight size={15} />
                </Button>
              </div>
            </article>
          ))}
        </div>
      )}
      {edit !== undefined && (
        <Dialog
          open
          onOpenChange={(v) => !v && setEdit(undefined)}
          title={edit ? "Shape your idea" : "Catch a little spark"}
        >
          <form
            className="form-stack"
            onSubmit={(e) => {
              e.preventDefault();
              void action(
                () =>
                  api(
                    edit ? `/ideas/${edit.id}` : "/ideas",
                    IdeaSchema,
                    edit ? "PATCH" : "POST",
                    edit
                      ? ({ title, text, color } satisfies UpdateIdeaRequest)
                      : ({
                          brand_id: brand.id,
                          title,
                          text,
                          color,
                        } satisfies CreateIdeaRequest),
                  ),
                "Idea saved",
              ).then((ok) => ok && setEdit(undefined));
            }}
          >
            <Field label="Idea title">
              <input
                autoFocus
                required
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                maxLength={240}
                placeholder="What’s on your mind?"
              />
            </Field>
            <Field label="Notes">
              <textarea
                rows={7}
                value={text}
                onChange={(e) => setText(e.target.value)}
                placeholder="A few words, a link, a whole story…"
              />
            </Field>
            <Field label="Color">
              <select value={color} onChange={(e) => setColor(e.target.value)}>
                <option value="lavender">Lavender</option>
                <option value="peach">Peach</option>
                <option value="mint">Mint</option>
                <option value="sky">Sky</option>
              </select>
            </Field>
            <div className="dialog-footer">
              {edit && (
                <Button
                  type="button"
                  variant="destructive"
                  disabled={pending}
                  onClick={() =>
                    void action(
                      () => api(`/ideas/${edit.id}`, ok, "DELETE"),
                      "Idea deleted",
                    ).then((ok) => ok && setEdit(undefined))
                  }
                >
                  <Trash2 size={16} />
                  Delete idea
                </Button>
              )}
              <Submit pending={pending}>Save idea</Submit>
            </div>
          </form>
        </Dialog>
      )}
    </>
  );
}
