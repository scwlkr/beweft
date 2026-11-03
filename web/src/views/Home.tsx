import { AlertTriangle, ArrowRight, CalendarDays, FileText, Lightbulb, Plus, Send } from "lucide-react";
import { Button } from "../components/ui/button";
import { State } from "../components/shared";
import { Network } from "../components/brand";
import type { Brand, Channel, Idea, Post, Publication } from "../api";
import { zone } from "../navigation";

type Props = {
  brand: Brand;
  channels: Channel[];
  posts: Post[];
  publications: Publication[];
  media: any[];
  ideas?: Idea[];
  onCompose: (initial?: Partial<Post>) => void;
  onGo: (route: string) => void;
};

function formatTime(ts?: string | null) {
  if (!ts) return "";
  try {
    const d = new Date(ts);
    return d.toLocaleString(undefined, { timeZone: zone, month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
  } catch {
    return ts;
  }
}

function startOfWeek(d: Date) {
  const x = new Date(d.getFullYear(), d.getMonth(), d.getDate());
  const day = (x.getDay() + 6) % 7;
  x.setDate(x.getDate() - day);
  x.setHours(0, 0, 0, 0);
  return x;
}
const dayNames = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const dayKey = (d: Date) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;

export function Home({ brand, channels, posts, publications, media, ideas = [], onCompose, onGo }: Props) {
  const attention = publications.filter((p) => ["failed", "uncertain", "manual_required"].includes(p.state));
  const queued = publications
    .filter((p) => !["published", "canceled", "fixture_complete", "rejected", "failed_terminal"].includes(p.state))
    .sort((a, b) => (a.scheduled_at || a.created_at).localeCompare(b.scheduled_at || b.created_at))
    .slice(0, 5);
  const drafts = posts
    .filter((p: any) => !(p.archived_at))
    .sort((a: any, b: any) => (b.updated_at || b.created_at).localeCompare(a.updated_at || a.created_at))
    .slice(0, 4);
  const ideaCount = ideas.length;
  const weekStart = startOfWeek(new Date());
  const weekDays = Array.from({ length: 7 }, (_, i) => {
    const d = new Date(weekStart);
    d.setDate(weekStart.getDate() + i);
    return d;
  });
  const weekCounts = weekDays.map(
    (d) => publications.filter((p) => p.scheduled_at && dayKey(new Date(p.scheduled_at)) === dayKey(d)).length,
  );

  return (
    <div className="home-grid">
      <section className="home-hero">
        <div>
          <div className="eyebrow">HOME</div>
          <h2>What's on deck for {brand.name}</h2>
          <p>Plan ahead, stay consistent, and publish with confidence across all your channels.</p>
          <div className="home-hero-actions">
            <Button onClick={() => onCompose()}>
              <Plus size={18} />
              Create post
            </Button>
            <Button variant="secondary" onClick={() => onGo("queue")}>
              <Send size={18} />
              View queue
            </Button>
            <Button variant="secondary" onClick={() => onGo("calendar")}>
              <CalendarDays size={18} />
              View calendar
            </Button>
            <Button variant="ghost" onClick={() => onGo("ideas")}>
              <Lightbulb size={18} />
              Capture idea
            </Button>
          </div>
        </div>
      </section>

      {attention.length > 0 && (
        <article className="card home-card home-attention">
          <header className="card-header home-card-header">
            <h3 className="card-title home-card-title">
              <AlertTriangle size={18} />
              Needs attention ({attention.length})
            </h3>
            <Button variant="ghost" size="sm" onClick={() => onGo("queue")}>
              Review in queue <ArrowRight size={14} />
            </Button>
          </header>
          <div className="card-content home-card-content">
            <ul className="home-list">
              {attention.slice(0, 4).map((p) => (
                <li key={p.id} className="home-list-item">
                  <div>
                    <div className="home-list-title">{p.payload.title || p.payload.text.slice(0, 60) || "Untitled post"}</div>
                    <div className="home-list-meta">
                      {p.channel_name} · {p.scheduled_at ? formatTime(p.scheduled_at) : ""}
                      {p.last_error ? ` — ${p.last_error.slice(0, 80)}` : ""}
                    </div>
                    <div className="home-list-meta"><State state={p.state} /></div>
                  </div>
                  <Network provider={p.provider} />
                </li>
              ))}
            </ul>
            {attention.length > 4 && (
              <p className="home-empty">+{attention.length - 4} more in the queue.</p>
            )}
          </div>
        </article>
      )}

      <div className="home-cards">
        <article className="card home-card">
          <header className="card-header home-card-header">
            <h3 className="card-title home-card-title">
              <Send size={18} />
              Upcoming
            </h3>
            <Button variant="ghost" size="sm" onClick={() => onGo("queue")}>
              See all <ArrowRight size={14} />
            </Button>
          </header>
          <div className="card-content home-card-content">
            {queued.length === 0 ? (
              <p className="home-empty">Nothing scheduled yet. Create your first post to get started.</p>
            ) : (
              <ul className="home-list">
                {queued.map((p) => (
                  <li key={p.id} className="home-list-item">
                    <div>
                      <div className="home-list-title">{p.payload.title || p.payload.text.slice(0, 60) || "Untitled post"}</div>
                      <div className="home-list-meta">
                        {p.channel_name} · {p.scheduled_at ? formatTime(p.scheduled_at) : p.state.replaceAll("_", " ")}
                      </div>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </article>

        <article className="card home-card">
          <header className="card-header home-card-header">
            <h3 className="card-title home-card-title">
              <FileText size={18} />
              Latest drafts
            </h3>
            <Button variant="ghost" size="sm" onClick={() => onGo("drafts")}>
              See all <ArrowRight size={14} />
            </Button>
          </header>
          <div className="card-content home-card-content">
            {drafts.length === 0 ? (
              <p className="home-empty">No drafts yet. Save ideas as drafts to polish later.</p>
            ) : (
              <ul className="home-list">
                {drafts.map((post: any) => (
                  <li key={post.id} className="home-list-item home-list-item-clickable" onClick={() => onCompose(post)}>
                    <div>
                      <div className="home-list-title">{post.title || (post.text || "").slice(0, 60) || "Untitled draft"}</div>
                      <div className="home-list-meta">Updated {formatTime(post.updated_at || post.created_at)}</div>
                    </div>
                    <ArrowRight size={16} className="home-list-arrow" />
                  </li>
                ))}
              </ul>
            )}
          </div>
        </article>

        <article className="card home-card">
          <header className="card-header home-card-header">
            <h3 className="card-title home-card-title">
              <Lightbulb size={18} />
              Ideas
            </h3>
            <Button variant="ghost" size="sm" onClick={() => onGo("ideas")}>
              Browse ideas <ArrowRight size={14} />
            </Button>
          </header>
          <div className="card-content home-card-content">
            {ideaCount === 0 ? (
              <>
                <p className="home-empty">Capture sparks in Ideas and turn them into posts when inspiration strikes.</p>
                <Button variant="secondary" onClick={() => onGo("ideas")}>
                  <Lightbulb size={16} />
                  Go to Ideas
                </Button>
              </>
            ) : (
              <>
                <div className="home-stats">
                  <div className="home-stat">
                    <div className="home-stat-value">{ideaCount}</div>
                    <div className="home-stat-label">Saved ideas</div>
                  </div>
                </div>
                <Button variant="secondary" onClick={() => onGo("ideas")}>
                  <Lightbulb size={16} />
                  View ideas
                </Button>
              </>
            )}
          </div>
        </article>

        <article className="card home-card">
          <header className="card-header home-card-header">
            <h3 className="card-title home-card-title">
              <CalendarDays size={18} />
              This week
            </h3>
            <Button variant="ghost" size="sm" onClick={() => onGo("calendar")}>
              Open calendar <ArrowRight size={14} />
            </Button>
          </header>
          <div className="card-content home-card-content">
            <div className="home-stats">
              <div className="home-stat">
                <div className="home-stat-value">{queued.length}</div>
                <div className="home-stat-label">Scheduled</div>
              </div>
              <div className="home-stat">
                <div className="home-stat-value">{drafts.length}</div>
                <div className="home-stat-label">Drafts</div>
              </div>
              <div className="home-stat">
                <div className="home-stat-value">{channels.length}</div>
                <div className="home-stat-label">Channels</div>
              </div>
            </div>
            <div className="home-week" aria-label="Posts scheduled per day this week">
              {weekDays.map((d, i) => (
                <button
                  key={dayKey(d)}
                  className={`home-week-day${dayKey(d) === dayKey(new Date()) ? " today" : ""}`}
                  onClick={() => onGo("calendar")}
                  aria-label={`${dayNames[i]} ${d.getDate()}: ${weekCounts[i]} posts`}
                >
                  <span>{dayNames[i]}</span>
                  <strong>{d.getDate()}</strong>
                  <small>{weekCounts[i]} {weekCounts[i] === 1 ? "post" : "posts"}</small>
                </button>
              ))}
            </div>
          </div>
        </article>
      </div>
    </div>
  );
}
