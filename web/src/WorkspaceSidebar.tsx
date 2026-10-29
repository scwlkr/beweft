import { ArrowUpRight, LogOut, Plus } from "lucide-react";
import { Unicorn } from "./components/brand";
import { Button } from "./components/ui/button";
import { useMobileDrawer } from "./useMobileDrawer";
import { routes } from "./navigation";
import { useWorkspace } from "./useWorkspace";
export function WorkspaceSidebar({
  workspace,
}: {
  workspace: ReturnType<typeof useWorkspace>;
}) {
  const {
    sidebar,
    brand,
    brands,
    brandId,
    chooseBrand,
    setBrandOpen,
    route,
    go,
    posts,
    health,
    signOut,
    publications,
  } = workspace;
  const { drawer, mobile } = useMobileDrawer(sidebar, () =>
    workspace.setSidebar(false),
  );
  const activePubs = publications.filter(
    (p) => !["published", "fixture_complete", "canceled"].includes(p.state),
  );
  return (
    <aside
      ref={drawer}
      inert={mobile && !sidebar}
      aria-hidden={mobile && !sidebar ? true : undefined}
      role={mobile && sidebar ? "dialog" : undefined}
      aria-modal={mobile && sidebar ? true : undefined}
      aria-label="Workspace navigation"
      className={`sidebar ${sidebar ? "sidebar-open" : ""}`}
    >
      <a className="wordmark" href="#queue" onClick={() => go("queue")}>
        <Unicorn size={38} />
        beweft<span className="wordmark-dot">✦</span>
      </a>
      <div className="brand-switcher">
        <span
          className="brand-avatar"
          style={{ background: brand?.color ?? "var(--accent)" }}
        >
          {brand?.name.slice(0, 1) ?? "✦"}
        </span>
        <label className="sr-only" htmlFor="brand-select">
          Current brand
        </label>
        <select
          id="brand-select"
          value={brandId}
          onChange={(e) => chooseBrand(e.target.value)}
        >
          {!brands.length && <option value="">Your workspace</option>}
          {brands.map((b) => (
            <option key={b.id} value={b.id}>
              {b.name}
            </option>
          ))}
        </select>
        <Button
          variant="ghost"
          size="icon"
          onClick={() => {
            workspace.setSidebar(false);
            setBrandOpen(true);
          }}
          aria-label="Create brand"
        >
          <Plus size={16} />
        </Button>
      </div>
      <nav aria-label="Main navigation">
        {routes.map((r) => (
          <button
            key={r.id}
            className={`nav-item ${r.group ? "nav-separator" : ""} ${route === r.id ? "active" : ""}`}
            aria-current={route === r.id ? "page" : undefined}
            onClick={() => go(r.id)}
          >
            <r.icon size={19} />
            <span>{r.name}</span>
            {r.id === "queue" && activePubs.length > 0 && (
              <b>{activePubs.length}</b>
            )}
            {r.id === "drafts" &&
              posts.filter((p) => p.status === "draft").length > 0 && (
                <b>{posts.filter((p) => p.status === "draft").length}</b>
              )}
          </button>
        ))}
      </nav>
      <div className="sidebar-bottom">
        <div className="workspace-status">
          <span
            className={`health-dot ${health?.worker_healthy ? "healthy" : ""}`}
          />
          <div>
            <strong>
              {health?.worker_healthy
                ? "Worker is online"
                : "Worker needs attention"}
            </strong>
            <small>
              {health?.worker_healthy
                ? "Ready when your posts are"
                : "Check your server worker process"}
            </small>
          </div>
        </div>
        <div className="sidebar-meta">
          <a
            href="https://github.com/scwlkr/beweft"
            target="_blank"
            rel="noreferrer"
          >
            Open source <ArrowUpRight size={12} />
          </a>
          <Button
            variant="ghost"
            size="icon"
            aria-label="Sign out"
            onClick={() => void signOut()}
          >
            <LogOut size={17} />
          </Button>
        </div>
      </div>
    </aside>
  );
}
