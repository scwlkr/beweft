import {
  ArrowUpRight,
  ExternalLink,
  Eye,
  EyeOff,
  GripVertical,
  Link2,
  Plus,
  Trash2,
} from "lucide-react";
import { useEffect, useState } from "react";
import { z } from "zod";
import { api, type Brand } from "../api";
import { Empty, ErrorNotice, Field, Submit } from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";

type Link = {
  id: string;
  start_page_id: string;
  title: string;
  url: string;
  position: number;
  enabled: boolean;
  clicks: number;
  created_at: string;
};

type StartPage = {
  id: string;
  brand_id: string;
  slug: string;
  title: string;
  bio: string;
  theme: string;
  avatar_media_id: string | null;
  published: boolean;
  created_at: string;
  updated_at: string;
  links: Link[];
};

const StartPageSchema = z.object({
  id: z.string(),
  brand_id: z.string(),
  slug: z.string(),
  title: z.string(),
  bio: z.string(),
  theme: z.string(),
  avatar_media_id: z.string().nullable(),
  published: z.boolean(),
  created_at: z.string(),
  updated_at: z.string(),
  links: z.array(
    z.object({
      id: z.string(),
      start_page_id: z.string(),
      title: z.string(),
      url: z.string(),
      position: z.number(),
      enabled: z.boolean(),
      clicks: z.number(),
      created_at: z.string(),
    }),
  ),
});

export function StartPages({
  brand,
  action,
  pending,
}: {
  brand: Brand;
  action: (fn: () => Promise<unknown>, message?: string) => Promise<boolean>;
  pending: boolean;
}) {
  const [page, setPage] = useState<StartPage | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [creating, setCreating] = useState(false);
  const [slug, setSlug] = useState("");
  const [title, setTitle] = useState("");
  const [bio, setBio] = useState("");
  const [linkForm, setLinkForm] = useState({ title: "", url: "" });

  async function load() {
    try {
      setLoading(true);
      setError("");
      const pages = await api(
        `/start-pages?brand_id=${encodeURIComponent(brand.id)}`,
        z.array(StartPageSchema),
      );
      setPage(pages[0] ?? null);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load start page");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void load();
  }, [brand.id]);

  async function createPage() {
    const body = {
      brand_id: brand.id,
      slug: slug.trim(),
      title: title.trim(),
      bio: bio.trim(),
      published: true,
    };
    const done = await action(
      () => api("/start-pages", StartPageSchema, "POST", body),
      "Start page created",
    );
    if (done) {
      setCreating(false);
      setSlug("");
      setTitle("");
      setBio("");
      void load();
    }
  }

  async function togglePublish() {
    if (!page) return;
    const done = await action(
      () =>
        api(`/start-pages/${page.id}`, StartPageSchema, "PATCH", {
          published: !page.published,
        }),
      page.published ? "Page unpublished" : "Page published",
    );
    if (done) void load();
  }

  async function addLink() {
    if (!page) return;
    const done = await action(
      () =>
        api(`/start-pages/${page.id}/links`, z.any(), "POST", {
          title: linkForm.title.trim(),
          url: linkForm.url.trim(),
          enabled: true,
        }),
      "Link added",
    );
    if (done) {
      setLinkForm({ title: "", url: "" });
      void load();
    }
  }

  async function toggleLink(link: Link) {
    if (!page) return;
    const done = await action(
      () =>
        api(`/start-pages/${page.id}/links/${link.id}`, z.any(), "PATCH", {
          enabled: !link.enabled,
        }),
      link.enabled ? "Link hidden" : "Link shown",
    );
    if (done) void load();
  }

  async function removeLink(link: Link) {
    if (!page) return;
    const done = await action(
      () => api(`/start-pages/${page.id}/links/${link.id}`, z.any(), "DELETE"),
      "Link removed",
    );
    if (done) void load();
  }

  if (loading) {
    return <div className="card">Loading start page...</div>;
  }

  if (!page) {
    return (
      <>
        <ErrorNotice message={error} />
        <div className="card" style={{ textAlign: "center", padding: "3rem" }}>
          <Link2 size={48} style={{ margin: "0 auto 1rem", opacity: 0.5 }} />
          <h2 style={{ marginBottom: "0.5rem" }}>Create your Start Page</h2>
          <p className="muted" style={{ marginBottom: "2rem" }}>
            Share one link that leads to all of your important links
          </p>
          <Button onClick={() => setCreating(true)}>
            <Plus size={18} />
            Create Start Page
          </Button>
        </div>
        <Dialog
          open={creating}
          title="Create Start Page"
          onClose={() => setCreating(false)}
        >
          <div className="stack">
            <Field label="Slug">
              <input
                value={slug}
                onChange={(e) => setSlug(e.target.value.toLowerCase())}
                placeholder="yourbrand"
                pattern="[a-z0-9-_]+"
              />
              <p className="muted" style={{ fontSize: "0.875rem" }}>
                Your page will be at /start/{slug || "yourbrand"}
              </p>
            </Field>
            <Field label="Page title">
              <input
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                placeholder="@yourbrand"
              />
            </Field>
            <Field label="Bio">
              <textarea
                value={bio}
                onChange={(e) => setBio(e.target.value)}
                placeholder="Tell people about yourself"
                rows={3}
              />
            </Field>
            <div style={{ display: "flex", gap: "0.5rem", justifyContent: "flex-end" }}>
              <Button variant="secondary" onClick={() => setCreating(false)}>
                Cancel
              </Button>
              <Submit
                onClick={createPage}
                disabled={pending || !slug.trim()}
              >
                Create
              </Submit>
            </div>
          </div>
        </Dialog>
      </>
    );
  }

  return (
    <>
      <ErrorNotice message={error} />
      <div className="section-toolbar">
        <div style={{ display: "flex", gap: "0.75rem", alignItems: "center" }}>
          <span
            className={`badge ${page.published ? "ok" : "muted"}`}
            style={{ padding: "0.25rem 0.75rem" }}
          >
            {page.published ? "Published" : "Unpublished"}
          </span>
          <a
            href={`/start/${page.slug}`}
            target="_blank"
            rel="noopener noreferrer"
            className="muted"
            style={{ display: "flex", alignItems: "center", gap: "0.25rem", fontSize: "0.875rem" }}
          >
            /start/{page.slug}
            <ExternalLink size={14} />
          </a>
        </div>
        <div style={{ display: "flex", gap: "0.5rem" }}>
          <Button variant="secondary" onClick={togglePublish} disabled={pending}>
            {page.published ? (
              <>
                <EyeOff size={16} /> Unpublish
              </>
            ) : (
              <>
                <Eye size={16} /> Publish
              </>
            )}
          </Button>
        </div>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "1.5rem" }}>
        <div className="card">
          <h2 style={{ marginBottom: "1.5rem" }}>Page Info</h2>
          <div className="stack">
            <Field label="Title">
              <input value={page.title} readOnly />
            </Field>
            <Field label="Bio">
              <textarea value={page.bio} readOnly rows={3} />
            </Field>
          </div>
        </div>

        <div className="card">
          <h2 style={{ marginBottom: "1.5rem" }}>Links</h2>
          <div className="stack" style={{ marginBottom: "1.5rem" }}>
            {page.links.length === 0 ? (
              <Empty message="Add your first link below" />
            ) : (
              page.links
                .sort((a, b) => a.position - b.position)
                .map((link) => (
                  <div
                    key={link.id}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: "0.75rem",
                      padding: "0.75rem",
                      border: "1px solid var(--border)",
                      borderRadius: "0.5rem",
                      opacity: link.enabled ? 1 : 0.5,
                    }}
                  >
                    <GripVertical size={16} className="muted" />
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{ fontWeight: 500 }}>{link.title}</div>
                      <div className="muted" style={{ fontSize: "0.875rem", overflow: "hidden", textOverflow: "ellipsis" }}>
                        {link.url}
                      </div>
                    </div>
                    <div className="muted" style={{ fontSize: "0.875rem" }}>
                      {link.clicks} clicks
                    </div>
                    <Button
                      variant="ghost"
                      size="icon"
                      onClick={() => toggleLink(link)}
                      disabled={pending}
                      title={link.enabled ? "Hide link" : "Show link"}
                    >
                      {link.enabled ? <Eye size={16} /> : <EyeOff size={16} />}
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      onClick={() => removeLink(link)}
                      disabled={pending}
                      title="Remove link"
                    >
                      <Trash2 size={16} />
                    </Button>
                  </div>
                ))
            )}
          </div>

          <div style={{ display: "grid", gap: "0.75rem", padding: "1rem", background: "var(--surface-2)", borderRadius: "0.5rem" }}>
            <Field label="Link title">
              <input
                value={linkForm.title}
                onChange={(e) => setLinkForm({ ...linkForm, title: e.target.value })}
                placeholder="My Website"
              />
            </Field>
            <Field label="URL">
              <input
                value={linkForm.url}
                onChange={(e) => setLinkForm({ ...linkForm, url: e.target.value })}
                placeholder="https://example.com"
              />
            </Field>
            <Button
              onClick={addLink}
              disabled={pending || !linkForm.title.trim() || !linkForm.url.trim()}
              style={{ justifySelf: "start" }}
            >
              <Plus size={16} />
              Add Link
            </Button>
          </div>
        </div>
      </div>
    </>
  );
}
