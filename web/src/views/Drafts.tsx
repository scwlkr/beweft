import { Feather, Plus, Trash2 } from "lucide-react";
import { useState } from "react";
import { api, ok, type Post } from "../api";
import { Empty, Time } from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";

type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;
export function Drafts({
  posts,
  action,
  pending,
  onEdit,
  onDuplicate,
  onCompose,
}: {
  posts: Post[];
  action: Action;
  pending: boolean;
  onEdit: (p: Post) => void;
  onDuplicate: (p: Post) => void;
  onCompose: () => void;
}) {
  const drafts = posts.filter((p) => p.status === "draft");
  const [confirm, setConfirm] = useState<Post | null>(null);
  return (
    <>
      {drafts.length === 0 ? (
        <Empty
          title="Your next post is waiting to happen"
          action={
            <Button onClick={onCompose}>
              <Plus size={17} />
              Start a draft
            </Button>
          }
        >
          Work on a post at your own pace. Your drafts stay here until you’re
          ready.
        </Empty>
      ) : (
        <div className="draft-list">
          {drafts.map((p) => (
            <article className="draft-row" key={p.id}>
              <div className="draft-icon">
                <Feather size={22} />
              </div>
              <button className="draft-content" onClick={() => onEdit(p)}>
                <h2>
                  {p.title || p.text.slice(0, 70) || "Untitled media post"}
                </h2>
                <p>{p.text.slice(0, 160)}</p>
                <small>
                  Updated <Time value={p.updated_at} /> · {p.variants.length}{" "}
                  channel variants
                </small>
              </button>
              <Button variant="secondary" size="sm" onClick={() => onEdit(p)}>
                Edit draft
              </Button>
              <Button variant="ghost" size="sm" onClick={() => onDuplicate(p)}>
                Duplicate
              </Button>
              <Button
                variant="ghost"
                size="icon"
                aria-label={`Delete ${p.title || "draft"}`}
                onClick={() => setConfirm(p)}
              >
                <Trash2 size={17} />
              </Button>
            </article>
          ))}
        </div>
      )}
      {confirm && (
        <Dialog
          open
          onOpenChange={(v) => !v && setConfirm(null)}
          title="Delete this draft?"
          description="This permanently removes the draft. Published history cannot be deleted here."
        >
          <div className="dialog-footer">
            <Button variant="secondary" onClick={() => setConfirm(null)}>
              Keep draft
            </Button>
            <Button
              variant="destructive"
              disabled={pending}
              onClick={() =>
                void action(
                  () => api(`/posts/${confirm.id}`, ok, "DELETE"),
                  "Draft deleted",
                ).then((ok) => ok && setConfirm(null))
              }
            >
              Delete draft
            </Button>
          </div>
        </Dialog>
      )}
    </>
  );
}
