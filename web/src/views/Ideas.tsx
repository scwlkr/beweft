import type {
  CreateIdeaGroupRequest,
  CreateIdeaRequest,
  IdeaStatus,
  UpdateIdeaGroupRequest,
  UpdateIdeaRequest,
} from "../lib/api-contract.generated";
import {
  ArrowUpRight,
  Lightbulb,
  MoreHorizontal,
  Plus,
  Trash2,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { z } from "zod";
import {
  api,
  captureWorkspaceSession,
  IdeaGroupSchema,
  IdeaSchema,
  isCurrentWorkspaceSession,
  ok,
  type Brand,
  type Idea,
  type IdeaGroup,
} from "../api";
import { Empty, ErrorNotice, Field, Submit } from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";

type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;
const stages: { id: IdeaStatus; label: string }[] = [
  { id: "inbox", label: "Inbox" },
  { id: "in_progress", label: "In progress" },
  { id: "ready", label: "Ready" },
  { id: "archived", label: "Archived" },
];
const colors = ["lavender", "peach", "mint", "sky"];

function GroupRow({
  group,
  action,
  pending,
  refresh,
}: {
  group: IdeaGroup;
  action: Action;
  pending: boolean;
  refresh: () => void;
}) {
  const [name, setName] = useState(group.name);
  const [color, setColor] = useState(group.color);
  useEffect(() => {
    setName(group.name);
    setColor(group.color);
  }, [group.name, group.color]);
  return (
    <div className="idea-group-row">
      <label className="sr-only" htmlFor={`group-${group.id}`}>
        Name for {group.name}
      </label>
      <input
        id={`group-${group.id}`}
        value={name}
        disabled={pending}
        maxLength={80}
        onChange={(event) => setName(event.target.value)}
      />
      <label className="sr-only" htmlFor={`group-color-${group.id}`}>
        Color for {group.name}
      </label>
      <select
        id={`group-color-${group.id}`}
        value={color}
        disabled={pending}
        onChange={(event) => setColor(event.target.value)}
      >
        {colors.map((item) => (
          <option key={item} value={item}>
            {item[0].toUpperCase() + item.slice(1)}
          </option>
        ))}
      </select>
      <Button
        type="button"
        variant="secondary"
        disabled={
          pending ||
          !name.trim() ||
          (name.trim() === group.name && color === group.color)
        }
        onClick={() => {
          const body = {
            name: name.trim(),
            color,
          } satisfies UpdateIdeaGroupRequest;
          void action(
            () =>
              api(`/idea-groups/${group.id}`, IdeaGroupSchema, "PATCH", body),
            "Group saved",
          ).then((done) => {
            if (done) refresh();
          });
        }}
      >
        Save
      </Button>
      <Button
        type="button"
        variant="ghost"
        disabled={pending}
        aria-label={`Delete group ${group.name}`}
        onClick={() => {
          void action(
            () => api(`/idea-groups/${group.id}`, ok, "DELETE"),
            "Group deleted; ideas kept",
          ).then((done) => {
            if (done) refresh();
          });
        }}
      >
        <Trash2 size={16} />
      </Button>
    </div>
  );
}

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
  const [status, setStatus] = useState<IdeaStatus>("inbox");
  const [groupId, setGroupId] = useState("");
  const [tagsInput, setTagsInput] = useState("");
  const [validation, setValidation] = useState("");
  const [groups, setGroups] = useState<IdeaGroup[]>([]);
  const [groupsReady, setGroupsReady] = useState(false);
  const [groupError, setGroupError] = useState("");
  const [groupFilter, setGroupFilter] = useState("all");
  const [tagFilter, setTagFilter] = useState("");
  const [manageGroups, setManageGroups] = useState(false);
  const [newGroup, setNewGroup] = useState("");
  const alive = useRef(false);
  const groupRequest = useRef<AbortController | null>(null);
  const loadGroups = useCallback(async () => {
    groupRequest.current?.abort();
    const controller = new AbortController();
    groupRequest.current = controller;
    let session;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    setGroupsReady(false);
    setGroupError("");
    try {
      const found = await api(
        `/idea-groups?brand_id=${encodeURIComponent(brand.id)}`,
        z.array(IdeaGroupSchema),
        "GET",
        undefined,
        controller.signal,
      );
      if (
        !alive.current ||
        controller.signal.aborted ||
        groupRequest.current !== controller ||
        !isCurrentWorkspaceSession(session)
      )
        return;
      if (found.some((group) => group.brand_id !== brand.id))
        throw new Error("Idea groups belong to a different brand.");
      setGroups(found);
      setGroupFilter((current) =>
        current === "all" ||
        current === "ungrouped" ||
        found.some((group) => group.id === current)
          ? current
          : "all",
      );
      setGroupsReady(true);
    } catch (error) {
      if (
        alive.current &&
        !controller.signal.aborted &&
        groupRequest.current === controller &&
        isCurrentWorkspaceSession(session)
      )
        setGroupError((error as Error).message);
    } finally {
      if (groupRequest.current === controller) groupRequest.current = null;
    }
  }, [brand.id]);
  useEffect(() => {
    alive.current = true;
    void loadGroups();
    return () => {
      alive.current = false;
      groupRequest.current?.abort();
    };
  }, [loadGroups]);
  const refreshGroups = () => void loadGroups();
  const open = (idea: Idea | null) => {
    setEdit(idea);
    setTitle(idea?.title ?? "");
    setText(idea?.text ?? "");
    setColor(idea?.color ?? "lavender");
    setStatus(idea?.status ?? "inbox");
    setGroupId(idea?.group_id ?? "");
    setTagsInput(idea?.tags.join("\n") ?? "");
    setValidation("");
  };
  const knownTags = [...new Set(ideas.flatMap((idea) => idea.tags))].sort(
    (a, b) => a.localeCompare(b),
  );
  const currentTag = knownTags.includes(tagFilter) ? tagFilter : "";
  const filtered = ideas.filter(
    (idea) =>
      (groupFilter === "all" ||
        (groupFilter === "ungrouped"
          ? !idea.group_id
          : idea.group_id === groupFilter)) &&
      (!currentTag || idea.tags.includes(currentTag)),
  );
  const submit = () => {
    const tags: string[] = [],
      seen = new Set<string>();
    for (const raw of tagsInput.split("\n")) {
      const tag = raw.trim();
      if (!tag || seen.has(tag.toLowerCase())) continue;
      if (Array.from(tag).length > 40) {
        setValidation("Each tag must be 40 characters or fewer.");
        return;
      }
      seen.add(tag.toLowerCase());
      tags.push(tag);
    }
    if (tags.length > 20) {
      setValidation("Use no more than 20 tags.");
      return;
    }
    setValidation("");
    const fields = {
      title,
      text,
      color,
      status,
      tags,
      ...(groupsReady ? { group_id: groupId || null } : {}),
    };
    const body = edit
      ? (fields satisfies UpdateIdeaRequest)
      : ({ ...fields, brand_id: brand.id } satisfies CreateIdeaRequest);
    void action(
      () =>
        api(
          edit ? `/ideas/${edit.id}` : "/ideas",
          IdeaSchema,
          edit ? "PATCH" : "POST",
          body,
        ),
      "Idea saved",
    ).then((done) => {
      if (done) setEdit(undefined);
    });
  };
  return (
    <>
      <div className="section-toolbar">
        <p className="muted">
          {ideas.length} {ideas.length === 1 ? "idea" : "ideas"} to explore
        </p>
        <Button
          variant="secondary"
          onClick={() => setManageGroups(true)}
          disabled={!groupsReady}
        >
          Manage groups
        </Button>
        <Button onClick={() => open(null)}>
          <Plus size={17} />
          Capture an idea
        </Button>
      </div>
      {groupError && (
        <div className="inline-actions">
          <ErrorNotice
            message={`Idea groups could not be loaded: ${groupError}`}
          />
          <Button variant="secondary" onClick={refreshGroups}>
            Retry groups
          </Button>
        </div>
      )}
      {ideas.length > 0 && (
        <div className="idea-filters">
          <Field label="Filter by group">
            <select
              aria-label="Filter by group"
              value={groupFilter}
              disabled={!groupsReady}
              onChange={(event) => setGroupFilter(event.target.value)}
            >
              <option value="all">All groups</option>
              <option value="ungrouped">Ungrouped</option>
              {groups.map((group) => (
                <option key={group.id} value={group.id}>
                  {group.name}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Filter by tag">
            <select
              aria-label="Filter by tag"
              value={currentTag ? `tag:${currentTag}` : ""}
              onChange={(event) => setTagFilter(event.target.value.slice(4))}
            >
              <option value="">All tags</option>
              {knownTags.map((tag) => (
                <option key={tag} value={`tag:${tag}`}>
                  {tag}
                </option>
              ))}
            </select>
          </Field>
        </div>
      )}
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
        <div className="idea-board" aria-label="Idea board">
          {stages.map((stage) => {
            const items = filtered.filter((idea) => idea.status === stage.id);
            return (
              <section
                className="idea-column"
                key={stage.id}
                aria-label={stage.label}
              >
                <h2>
                  {stage.label} <span className="muted">{items.length}</span>
                </h2>
                {items.length === 0 ? (
                  <p className="muted idea-empty">No ideas here yet.</p>
                ) : (
                  items.map((idea) => (
                    <article
                      className={`idea-card idea-${idea.color}`}
                      key={idea.id}
                    >
                      <div className="idea-card-top">
                        <Lightbulb size={18} />
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label={`Edit ${idea.title}`}
                          onClick={() => open(idea)}
                        >
                          <MoreHorizontal size={18} />
                        </Button>
                      </div>
                      <button
                        className="idea-content"
                        onClick={() => open(idea)}
                      >
                        <h3>{idea.title}</h3>
                        <p>{idea.text}</p>
                      </button>
                      {idea.group_id && (
                        <small className="idea-group-label">
                          {groups.find((group) => group.id === idea.group_id)
                            ?.name ?? "Group unavailable"}
                        </small>
                      )}
                      {idea.tags.length > 0 && (
                        <div className="idea-tags">
                          {idea.tags.map((tag) => (
                            <span key={tag}>{tag}</span>
                          ))}
                        </div>
                      )}
                      <label
                        className="sr-only"
                        htmlFor={`idea-stage-${idea.id}`}
                      >
                        Move {idea.title}
                      </label>
                      <select
                        id={`idea-stage-${idea.id}`}
                        value={idea.status}
                        disabled={pending}
                        onChange={(event) => {
                          const next = event.target.value as IdeaStatus;
                          void action(
                            () =>
                              api(`/ideas/${idea.id}`, IdeaSchema, "PATCH", {
                                status: next,
                              } satisfies UpdateIdeaRequest),
                            "Idea moved",
                          );
                        }}
                      >
                        {stages.map((stage) => (
                          <option key={stage.id} value={stage.id}>
                            {stage.label}
                          </option>
                        ))}
                      </select>
                      <div className="idea-card-bottom">
                        <small>
                          {new Date(idea.updated_at).toLocaleDateString(
                            undefined,
                            { month: "short", day: "numeric" },
                          )}
                        </small>
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => onCompose(idea)}
                        >
                          Create post <ArrowUpRight size={15} />
                        </Button>
                      </div>
                    </article>
                  ))
                )}
              </section>
            );
          })}
        </div>
      )}
      {manageGroups && (
        <Dialog
          open
          onOpenChange={(open) => !open && setManageGroups(false)}
          title="Idea groups"
          description="Organize ideas by theme. Deleting a group keeps its ideas."
        >
          <div className="form-stack">
            <form
              className="inline-actions"
              onSubmit={(event) => {
                event.preventDefault();
                const name = newGroup.trim();
                if (!name) return;
                const body = {
                  brand_id: brand.id,
                  name,
                  position: groups.length,
                } satisfies CreateIdeaGroupRequest;
                void action(
                  () => api("/idea-groups", IdeaGroupSchema, "POST", body),
                  "Group created",
                ).then((done) => {
                  if (done) {
                    setNewGroup("");
                    refreshGroups();
                  }
                });
              }}
            >
              <Field label="New group name">
                <input
                  value={newGroup}
                  disabled={pending}
                  maxLength={80}
                  onChange={(event) => setNewGroup(event.target.value)}
                />
              </Field>
              <Button type="submit" disabled={pending || !newGroup.trim()}>
                Add group
              </Button>
            </form>
            {groups.length === 0 ? (
              <p className="muted">No groups yet.</p>
            ) : (
              groups.map((group) => (
                <GroupRow
                  key={group.id}
                  group={group}
                  action={action}
                  pending={pending}
                  refresh={refreshGroups}
                />
              ))
            )}
          </div>
        </Dialog>
      )}
      {edit !== undefined && (
        <Dialog
          open
          onOpenChange={(open) => !open && setEdit(undefined)}
          title={edit ? "Shape your idea" : "Catch a little spark"}
        >
          <form
            className="form-stack"
            onSubmit={(event) => {
              event.preventDefault();
              submit();
            }}
          >
            <Field label="Idea title">
              <input
                autoFocus
                required
                disabled={pending}
                value={title}
                onChange={(event) => setTitle(event.target.value)}
                maxLength={240}
                placeholder="What’s on your mind?"
              />
            </Field>
            <Field label="Notes">
              <textarea
                rows={7}
                aria-label="Notes"
                disabled={pending}
                value={text}
                onChange={(event) => setText(event.target.value)}
                placeholder="A few words, a link, a whole story…"
              />
            </Field>
            <Field label="Stage">
              <select
                aria-label="Stage"
                value={status}
                disabled={pending}
                onChange={(event) =>
                  setStatus(event.target.value as IdeaStatus)
                }
              >
                {stages.map((stage) => (
                  <option key={stage.id} value={stage.id}>
                    {stage.label}
                  </option>
                ))}
              </select>
            </Field>
            <Field label="Group">
              <select
                aria-label="Group"
                value={groupId}
                disabled={pending || !groupsReady}
                onChange={(event) => setGroupId(event.target.value)}
              >
                <option value="">Ungrouped</option>
                {groups.map((group) => (
                  <option key={group.id} value={group.id}>
                    {group.name}
                  </option>
                ))}
              </select>
            </Field>
            {!groupsReady && (
              <small className="muted">
                Groups are unavailable; an existing group stays unchanged.
              </small>
            )}
            <Field label="Tags">
              <textarea
                rows={3}
                aria-label="Tags"
                aria-describedby="idea-tags-help"
                disabled={pending}
                value={tagsInput}
                onChange={(event) => setTagsInput(event.target.value)}
              />
            </Field>
            <small id="idea-tags-help" className="muted">
              One per line; up to 20 tags, 40 characters each.
            </small>
            <Field label="Color">
              <select
                aria-label="Color"
                disabled={pending}
                value={color}
                onChange={(event) => setColor(event.target.value)}
              >
                {colors.map((item) => (
                  <option key={item} value={item}>
                    {item[0].toUpperCase() + item.slice(1)}
                  </option>
                ))}
              </select>
            </Field>
            {validation && <ErrorNotice message={validation} />}
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
                    ).then((done) => done && setEdit(undefined))
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
