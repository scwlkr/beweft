import { ArrowDown, ArrowUp, CalendarDays, Copy, Download, FileUp, Image, Plus } from "lucide-react";
import { useRef, useState } from "react";
import { z } from "zod";
import {
  api,
  PublicationSchema,
  PostSchema,
  type Channel,
  type Media,
  type Publication,
  type Post,
  type ApiSession,
} from "../api";
import { Network } from "../components/brand";
import { Empty, ExternalLink, Field, State, Time } from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";

import type { PendingMedia } from "../usePendingMedia";
import type { usePendingSchedule } from "../usePendingSchedule";
import { PublicationDetails } from "./PublicationDetails";
import { Calendar } from "./Calendar";
type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;
export function Queue({
  publications,
  allPublications,
  channels,
  media,
  posts,
  pendingSchedule,
  pendingMedia,
  reconcileMedia,
  onEdit,
  onRefresh,
  action,
  pending,
  onCompose,
  brandId,
}: {
  publications: Publication[];
  allPublications: Publication[];
  channels: Channel[];
  media: Media[];
  posts: Post[];
  pendingSchedule: ReturnType<typeof usePendingSchedule>;
  pendingMedia: PendingMedia;
  reconcileMedia: (media: Media, session: ApiSession) => void;
  onEdit: (post: Post) => void;
  onRefresh: () => Promise<void>;
  action: Action;
  pending: boolean;
  onCompose: () => void;
  brandId?: string;
}) {
  const csvBrandId = brandId ?? channels[0]?.brand_id ?? "";
  const brandChannels = channels.filter((c) => c.brand_id === csvBrandId);
  const [view, setView] = useState("upcoming");
  const [surface, setSurface] = useState<"list" | "calendar">("list");
  const [tagFilter, setTagFilter] = useState("");
  const [channel, setChannel] = useState("all");
  const [status, setStatus] = useState("all");
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<"newest" | "oldest">("newest");
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const exportCsv = () => {
    const rows = [
      ["id", "channel", "state", "scheduled_at", "text"].join(","),
      ...filtered.map((p) =>
        [p.id, p.channel_name, p.state, p.scheduled_at, JSON.stringify(p.payload.text)].join(","),
      ),
    ].join("\\n");
    const blob = new Blob([rows], { type: "text/csv" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "queue.csv";
    a.click();
    URL.revokeObjectURL(url);
  };
  const [selected, setSelected] = useState<Publication | null>(null);
  const [bulkRescheduleOpen, setBulkRescheduleOpen] = useState(false);
  const [bulkRescheduleAt, setBulkRescheduleAt] = useState("");
  const importRef = useRef<HTMLInputElement>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [importChannel, setImportChannel] = useState("");
  const [importResult, setImportResult] = useState("");
  const [importing, setImporting] = useState(false);
  const history = ["published", "fixture_complete", "canceled"];
  const statusOptions = [
    { value: "all", label: "All statuses" },
    { value: "queued", label: "Queued" },
    { value: "sending", label: "Sending" },
    { value: "published", label: "Published" },
    { value: "failed", label: "Failed" },
    { value: "uncertain", label: "Uncertain" },
    { value: "manual_required", label: "Needs handoff" },
  ];
  const tagMap = new Map(
    publications.map((p) => {
      const tags = new Set<string>();
      const add = (value: unknown) => {
        if (Array.isArray(value)) {
          for (const item of value) {
            if (typeof item === "string" && item.trim())
              tags.add(item.trim());
          }
        }
      };
      add((p as { tags?: unknown }).tags);
      add((p.payload as { tags?: unknown })?.tags);
      add(p.payload?.options?.tags);
      const post = posts.find((x) => x.id === p.post_id);
      if (post) {
        add((post as { tags?: unknown }).tags);
        add(post.options?.tags);
      }
      return [p.id, [...tags]] as const;
    }),
  );
  const knownTags = [
    ...new Set(publications.flatMap((p) => tagMap.get(p.id) ?? [])),
  ].sort((a, b) => a.localeCompare(b));
  const filtered = publications
    .filter(
      (p) =>
        (channel === "all" || p.channel_id === channel) &&
        (status === "all" || p.state === status) &&
        (search === "" ||
          p.payload.text.toLowerCase().includes(search.toLowerCase()) ||
          (p.payload.title ?? "").toLowerCase().includes(search.toLowerCase())) &&
        (view === "history"
        ? history.includes(p.state)
        : view === "attention"
          ? ["failed", "uncertain", "manual_required"].includes(p.state)
          : !history.includes(p.state)) &&
        (tagFilter === "" ||
          (tagFilter === "__untagged__"
            ? (tagMap.get(p.id) ?? []).length === 0
            : (tagMap.get(p.id) ?? []).includes(tagFilter))),
    ).sort((a, b) =>
    sort === "newest"
      ? b.scheduled_at.localeCompare(a.scheduled_at)
      : a.scheduled_at.localeCompare(b.scheduled_at),
  );
  const reorder = async (p: Publication, direction: number) => {
    const queue = allPublications
      .filter(
        (x) =>
          x.channel_id === p.channel_id &&
          x.state === "queued" &&
          x.scheduling_mode === "queue",
      )
      .sort((a, b) => a.scheduled_at.localeCompare(b.scheduled_at));
    const index = queue.findIndex((x) => x.id === p.id);
    if (index + direction < 0 || index + direction >= queue.length) return;
    [queue[index], queue[index + direction]] = [
      queue[index + direction],
      queue[index],
    ];
    await action(
      () =>
        api("/queue/reorder", z.array(PublicationSchema), "POST", {
          channel_id: p.channel_id,
          publication_ids: queue.map((x) => x.id),
        }),
      "Queue reordered",
    );
  };
  if (surface === "calendar") {
    return (
      <>
        <div className="view-toggle" role="group" aria-label="Queue surface" style={{ marginBottom: 12 }}>
          <Button variant="secondary" size="sm" onClick={() => setSurface("list")}>
            List
          </Button>
          <Button variant="ghost" size="sm" onClick={() => setSurface("calendar")}>
            Calendar
          </Button>
        </div>
        <Calendar
          publications={publications}
          channels={channels}
          onCompose={() => onCompose()}
          action={action}
          onRefresh={onRefresh}
          posts={posts}
        />
      </>
    );
  }
  return (
    <>
      <div className="view-toggle" role="group" aria-label="Queue surface" style={{ marginBottom: 12 }}>
        <Button variant="secondary" size="sm" onClick={() => setSurface("list")}>
          List
        </Button>
        <Button variant="ghost" size="sm" onClick={() => setSurface("calendar")}>
          Calendar
        </Button>
      </div>
      <div className="view-toolbar">
        <div className="tabs">
          <button
            aria-pressed={view === "upcoming"}
            onClick={() => setView("upcoming")}
          >
            Queue{" "}
            <span>
              {publications.filter((p) => !history.includes(p.state)).length}
            </span>
          </button>
          <button
            aria-pressed={view === "attention"}
            onClick={() => setView("attention")}
          >
            Needs attention
            {publications.filter((p) => ["failed", "uncertain", "manual_required"].includes(p.state)).length > 0 && (
              <span>
                {publications.filter((p) => ["failed", "uncertain", "manual_required"].includes(p.state)).length}
              </span>
            )}
          </button>
          <button
            aria-pressed={view === "history"}
            onClick={() => setView("history")}
          >
            Sent
          </button>
        </div>
        <label className="sr-only" htmlFor="channel-filter">
          Filter channel
        </label>
        <select
          id="channel-filter"
          value={channel}
          onChange={(e) => setChannel(e.target.value)}
        >
          <option value="all">All channels</option>
          {channels.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
        <select value={status} onChange={(e) => setStatus(e.target.value)} aria-label="Filter by status">
          {statusOptions.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>
        <select value={sort} onChange={(e) => setSort(e.target.value as "newest" | "oldest")} aria-label="Sort posts">
          <option value="newest">Newest first</option>
          <option value="oldest">Oldest first</option>
        </select>
        <select value={tagFilter} onChange={(e) => setTagFilter(e.target.value)} aria-label="Filter by tag">
          <option value="">All tags</option>
          <option value="__untagged__">Untagged</option>
          {knownTags.map((tag) => (
            <option key={tag} value={tag}>
              {tag}
            </option>
          ))}
        </select>
        <button type="button" className="text-link" onClick={exportCsv} title="Export queue as CSV">
          <Download size={14} /> Export CSV
        </button>
        <input
          type="search"
          placeholder="Search posts…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          aria-label="Search posts"
        />
        <Button
          size="sm"
          onClick={onCompose}
          title="Create a new post"
        >
          <Plus size={16} />
          New post
        </Button>
        <Button
          variant="secondary"
          size="sm"
          onClick={() => {
            setImportChannel(brandChannels[0]?.id ?? "");
            setImportResult("");
            setImportOpen(true);
          }}
          title="Import from CSV to create drafts"
        >
          <FileUp size={16} />
          Import CSV
        </Button>
        {importOpen && (
          <Dialog open onOpenChange={(v) => !v && !importing && setImportOpen(false)} title="Import CSV drafts" description="Create one draft per row. Use a header row with text (or message/content/caption) and an optional title column. Up to 50 rows per import. Rows are never published — review drafts before scheduling.">
            <div className="form-stack">
              <Field label="Target channel">
                <select value={importChannel} onChange={(e) => setImportChannel(e.target.value)} disabled={importing}>
                  {brandChannels.length === 0 && <option value="">No channels for this brand</option>}
                  {brandChannels.map((c) => (
                    <option key={c.id} value={c.id}>{c.name}</option>
                  ))}
                </select>
              </Field>
              <div className="inline-actions">
                <Button
                  variant="secondary"
                  size="sm"
                  disabled={importing}
                  onClick={() => {
                    const sample = "title,text\nHello world,\"First imported draft\"\n,Second draft without a title\n";
                    const blob = new Blob([sample], { type: "text/csv;charset=utf-8" });
                    const url = URL.createObjectURL(blob);
                    const a = document.createElement("a");
                    a.href = url; a.download = "beweft-import-template.csv"; a.click();
                    URL.revokeObjectURL(url);
                  }}
                >
                  <Download size={14} /> Download template
                </Button>
              </div>
              <input
                ref={importRef}
                type="file"
                accept=".csv,text/csv"
                disabled={importing}
                aria-label="CSV file to import"
                onChange={async (e) => {
                  const file = e.target.files?.[0];
                  e.target.value = "";
                  if (!file) return;
                  if (!importChannel) { setImportResult("Pick a target channel first."); return; }
                  if (!csvBrandId) { setImportResult("No brand selected — cannot import."); return; }
                  setImporting(true);
                  setImportResult("");
                  let created = 0, skipped = 0;
                  const failures: string[] = [];
                  try {
                    const text = await file.text();
                    const lines = text.split(/\r?\n/).filter((l) => l.trim());
                    if (lines.length === 0) { setImportResult("That CSV is empty."); setImporting(false); return; }
                    const parseRow = (line: string) => line.split(/,(?=(?:[^"]*"[^"]*")*[^"]*$)/).map((p) => p.trim().replace(/^"|"$/g, "").replaceAll('""', '"'));
                    const headers = parseRow(lines[0]).map((h) => h.toLowerCase());
                    const hasHeader = headers.some((h) => h.includes("text") || h.includes("message") || h.includes("content") || h.includes("caption") || h.includes("title"));
                    const textIdx = headers.findIndex((h) => h.includes("text") || h.includes("message") || h.includes("content") || h.includes("caption"));
                    const titleIdx = headers.findIndex((h) => h.includes("title"));
                    const start = hasHeader ? 1 : 0;
                    const count = Math.min(lines.length - start, 50);
                    if (count <= 0) { setImportResult("No data rows found — the template includes a header row plus data rows."); setImporting(false); return; }
                    for (let i = start; i < start + count; i++) {
                      const parts = parseRow(lines[i]);
                      const postText = hasHeader && textIdx >= 0 ? parts[textIdx] || "" : hasHeader ? "" : parts[0] || "";
                      const postTitle = hasHeader && titleIdx >= 0 ? parts[titleIdx] || "" : "";
                      if (!postText.trim() && !postTitle.trim()) { skipped++; continue; }
                      try {
                        await api("/posts", PostSchema, "POST", {
                          brand_id: csvBrandId,
                          title: postTitle,
                          text: postText,
                          media_ids: [],
                          variants: [{ channel_id: importChannel }],
                          options: {},
                        });
                        created++;
                      } catch (err) {
                        failures.push(`Row ${i + 1}: ${(err as Error).message}`);
                      }
                    }
                    await onRefresh();
                    setImportResult(
                      `Imported ${created} draft${created === 1 ? "" : "s"}${skipped ? `, skipped ${skipped} empty row${skipped === 1 ? "" : "s"}` : ""}${failures.length ? `. ${failures.length} failed: ${failures.slice(0, 3).join("; ")}${failures.length > 3 ? "…" : ""}` : ""}.`,
                    );
                  } catch (err) {
                    setImportResult(`Import failed: ${(err as Error).message}`);
                  } finally {
                    setImporting(false);
                  }
                }}
              />
              {importResult && <p className="availability-note" role="status">{importResult}</p>}
              <div className="inline-actions">
                <Button variant="secondary" disabled={importing} onClick={() => setImportOpen(false)}>Close</Button>
              </div>
            </div>
          </Dialog>
        )}
      </div>
      {selectedIds.size > 0 && (
        <div className="bulk-actions">
          <span>{selectedIds.size} selected</span>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              if (selectedIds.size === filtered.length) {
                setSelectedIds(new Set());
              } else {
                setSelectedIds(new Set(filtered.map((p) => p.id)));
              }
            }}
          >
            {selectedIds.size === filtered.length ? "Deselect all" : "Select all"}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setSelectedIds(new Set())}
          >
            Clear selection
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={async () => {
              const targets = filtered.filter(
                (p) =>
                  selectedIds.has(p.id) &&
                  p.state === "queued" &&
                  p.scheduling_mode === "queue" &&
                  channels.some((c) => c.id === p.channel_id),
              );
              if (targets.length === 0) {
                setSelectedIds(new Set());
                return;
              }
              const channelId = targets[0].channel_id;
              const queue = allPublications
                .filter(
                  (x) =>
                    x.channel_id === channelId &&
                    x.state === "queued" &&
                    x.scheduling_mode === "queue",
                )
                .sort((a, b) => a.scheduled_at.localeCompare(b.scheduled_at));
              const others = queue.filter((x) => !targets.some((t) => t.id === x.id));
              await action(
                () =>
                  api("/queue/reorder", z.array(PublicationSchema), "POST", {
                    channel_id: channelId,
                    publication_ids: [...targets.map((t) => t.id), ...others.map((o) => o.id)],
                  }),
                "Moved selected to front",
              );
              setSelectedIds(new Set());
              await onRefresh();
            }}
          >
            Move to front
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={async () => {
              const failed = filtered.filter(
                (p) => selectedIds.has(p.id) && ["failed", "uncertain", "manual_required"].includes(p.state),
              );
              if (failed.length === 0) {
                setSelectedIds(new Set());
                return;
              }
              let done = 0;
              for (const p of failed) {
                const ok = await action(
                  () => api(`/publications/${p.id}/retry`, PublicationSchema, "POST"),
                  "Retrying publication",
                );
                if (ok) done++;
              }
              setSelectedIds(new Set());
              await onRefresh();
              if (done > 0) {
                // Action already shows message; keep simple
              }
            }}
            title="Retry failed/needs attention"
          >
            Retry
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={() => {
              setSelectedIds(
                new Set(
                  filtered
                    .filter((p) => ["failed", "uncertain", "manual_required"].includes(p.state))
                    .map((p) => p.id),
                ),
              );
            }}
            title="Select all needing attention in current view"
          >
            Select failed
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={() => {
              const targets = filtered.filter(
                (p) => selectedIds.has(p.id) && ["queued", "manual_required"].includes(p.state),
              );
              if (targets.length === 0) {
                setSelectedIds(new Set());
                return;
              }
              const now = new Date(Date.now() + 3600000);
              const iso = new Date(now.getTime() - now.getTimezoneOffset() * 60000)
                .toISOString()
                .slice(0, 16);
              setBulkRescheduleAt(iso);
              setBulkRescheduleOpen(true);
            }}
            title="Reschedule selected"
          >
            Reschedule
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={async () => {
              const targets = filtered.filter((p) => selectedIds.has(p.id));
              if (targets.length === 0) {
                setSelectedIds(new Set());
                return;
              }
              let done = 0;
              for (const p of targets) {
                try {
                  await api("/posts", PostSchema, "POST", {
                    brand_id: p.brand_id,
                    title: p.payload.title ? `${p.payload.title} (copy)` : "",
                    text: p.payload.text ?? "",
                    media_ids: p.payload.media_ids ?? [],
                    variants: [{ channel_id: p.channel_id }],
                    options: p.payload.options ?? {},
                  });
                  done++;
                } catch {
                  // continue
                }
              }
              setSelectedIds(new Set());
              await onRefresh();
            }}
            title="Duplicate selected as drafts"
          >
            Duplicate
          </Button>
        </div>
      )}
      {filtered.length === 0 ? (
        <Empty
          title={
            view === "history"
              ? "Your published posts will appear here"
              : view === "attention"
                ? "All clear for now"
                : "A little space for your next big idea"
          }
          action={
            view === "upcoming" ? (
              <Button onClick={onCompose}>
                <Plus size={17} />
                Create your first post
              </Button>
            ) : view === "attention" ? (
              <Button variant="secondary" onClick={() => setView("upcoming")}>
                View queue
              </Button>
            ) : undefined
          }
        >
          {view === "history"
            ? "Publishing outcomes and receipts will appear here after your posts go live."
            : view === "attention"
              ? "Posts that need a handoff, reconnect, or reconciliation will appear here."
              : "Create a post, choose your channels, and find its place in the schedule."}
        </Empty>
      ) : (
        <div className="publication-list">
          {filtered.map((p) => (
            <article className="publication-card" key={p.id}>
              <label className="publication-select">
                <input
                  type="checkbox"
                  checked={selectedIds.has(p.id)}
                  onChange={(e) => {
                    const next = new Set(selectedIds);
                    if (e.target.checked) next.add(p.id);
                    else next.delete(p.id);
                    setSelectedIds(next);
                  }}
                  aria-label="Select post"
                />
              </label>
              <div className="publication-time">
                <CalendarDays size={16} />
                <Time value={p.scheduled_at} />
              </div>
              <div className="publication-main">
                <div className="publication-author">
                  <Network provider={p.provider} />
                  <strong>{p.channel_name}</strong>
                  <State state={p.state} />
                  {channels.find((c) => c.id === p.channel_id)?.paused && (
                    <span className="status">Channel paused</span>
                  )}
                </div>
                {p.payload.title && <h3>{p.payload.title}</h3>}
                <p className="post-copy">{p.payload.text}</p>
                <button
                  type="button"
                  className="text-link"
                  onClick={() => navigator.clipboard?.writeText(p.payload.text)}
                  title="Copy post text"
                >
                  <Copy size={14} /> Copy text
                </button>
                {p.payload.media_ids.length > 0 && (
                  <div className="post-attachments">
                    {p.payload.media_ids.map((id) => {
                      const m = media.find((m) => m.id === id);
                      return m?.mime.startsWith("image/") ? (
                        <img src={m.url} alt={m.name} key={id} />
                      ) : (
                        <span key={id}>
                          <Image size={15} />
                          {m?.name ?? "Attached media"}
                        </span>
                      );
                    })}
                  </div>
                )}
                {p.last_error && <p className="inline-error">{p.last_error}</p>}
                <div className="publication-actions">
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => setSelected(p)}
                  >
                    {p.state === "manual_required"
                      ? "Complete handoff"
                      : p.state === "uncertain"
                        ? "Reconcile outcome"
                        : "View details"}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => {
                      onEdit({
                        title: p.payload.title ?? "",
                        text: p.payload.text ?? "",
                        id: undefined,
                      } as any);
                    }}
                    title="Duplicate as draft"
                  >
                    Duplicate
                  </Button>
                  {p.external_url && (
                    <ExternalLink url={p.external_url}>View post</ExternalLink>
                  )}
                  {p.state === "queued" && p.scheduling_mode === "queue" && (
                    <div className="reorder-buttons">
                      <Button
                        variant="secondary"
                        size="sm"
                        disabled={pending}
                        onClick={() =>
                          void action(
                            () =>
                              api(
                                `/publications/${p.id}/prioritize`,
                                z.array(PublicationSchema),
                                "POST",
                                { scheduled_at: p.scheduled_at },
                              ),
                            "Moved to the first queue slot",
                          ).then(async (done) => {
                            if (!done) {
                              await onRefresh();
                              setSelected(p);
                            }
                          })
                        }
                      >
                        Move to front
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        aria-label={`Move ${p.payload.title || "post"} earlier`}
                        disabled={pending}
                        onClick={() => void reorder(p, -1)}
                      >
                        <ArrowUp size={15} />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        aria-label={`Move ${p.payload.title || "post"} later`}
                        disabled={pending}
                        onClick={() => void reorder(p, 1)}
                      >
                        <ArrowDown size={15} />
                      </Button>
                    </div>
                  )}
                </div>
              </div>
            </article>
          ))}
        </div>
      )}
      {bulkRescheduleOpen && (
        <Dialog
          open
          onOpenChange={(v) => !v && !pending && setBulkRescheduleOpen(false)}
          title="Reschedule selected"
          description="Apply a new time to queued/manual posts in your selection."
        >
          <div className="form-stack">
            <Field label="New scheduled time">
              <input
                type="datetime-local"
                value={bulkRescheduleAt}
                onChange={(e) => setBulkRescheduleAt(e.target.value)}
              />
            </Field>
            <div className="inline-actions">
              <Button
                disabled={pending || !bulkRescheduleAt}
                onClick={async () => {
                  const targets = filtered.filter(
                    (p) =>
                      selectedIds.has(p.id) &&
                      ["queued", "manual_required"].includes(p.state),
                  );
                  if (targets.length === 0) {
                    setBulkRescheduleOpen(false);
                    setSelectedIds(new Set());
                    return;
                  }
                  const iso = new Date(bulkRescheduleAt).toISOString();
                  let done = 0;
                  for (const p of targets) {
                    const ok = await action(
                      () =>
                        api(`/publications/${p.id}/reschedule`, PublicationSchema, "POST", {
                          scheduled_at: iso,
                        }),
                      "Rescheduled",
                    );
                    if (ok) done++;
                  }
                  setBulkRescheduleOpen(false);
                  setSelectedIds(new Set());
                  await onRefresh();
                }}
              >
                Apply
              </Button>
              <Button
                variant="secondary"
                onClick={() => setBulkRescheduleOpen(false)}
                disabled={pending}
              >
                Cancel
              </Button>
            </div>
          </div>
        </Dialog>
      )}
      {selected && (
        <PublicationDetails
          media={media}
          post={posts.find((post) => post.id === selected.post_id)}
          pendingSchedule={pendingSchedule}
          pendingMedia={pendingMedia}
          reconcileMedia={reconcileMedia}
          onEdit={onEdit}
          onRefresh={onRefresh}
          channel={channels.find(
            (channel) => channel.id === selected.channel_id,
          )}
          publication={
            publications.find((p) => p.id === selected.id) ?? selected
          }
          onClose={() => setSelected(null)}
          action={action}
          pending={pending}
        />
      )}
    </>
  );
}
