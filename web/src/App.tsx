import {
  Check,
  ChevronRight,
  Clock3,
  Menu,
  Plus,
  RefreshCw,
  Search,
  X,
} from "lucide-react";
import { Auth } from "./Auth";
import { BrandDialog } from "./BrandDialog";
import { Unicorn } from "./components/brand";
import { ApiFeedbackContext } from "./components/feedback";
import { Busy, Empty, ErrorNotice } from "./components/shared";
import { Button } from "./components/ui/button";
import { Composer } from "./Composer";
import { titles, zone } from "./navigation";
import { useWorkspace } from "./useWorkspace";
import { Calendar } from "./views/Calendar";
import { ConnectionFlow } from "./views/ConnectionFlow";
import { Connections } from "./views/Connections";
import { Drafts } from "./views/Drafts";
import { Ideas } from "./views/Ideas";
import { Insights } from "./views/Insights";
import { MediaLibrary } from "./views/MediaLibrary";
import { Queue } from "./views/Queue";
import { Settings } from "./views/Settings";
import { WorkspaceSidebar } from "./WorkspaceSidebar";
export default function App() {
  const workspace = useWorkspace();
  const {
    ready,
    acceptSession,
    signingOut,
    signOutError,
    route,
    brands,
    brandId,
    channels,
    posts,
    ideas,
    media,
    publications,
    providers,
    health,
    loading,
    error,
    toast,
    pending,
    sidebar,
    setSidebar,
    composer,
    setComposer,
    brandOpen,
    setBrandOpen,
    search,
    setSearch,
    theme,
    setTheme,
    brand,
    load,
    action,
    go,
    createBrand,
  } = workspace;
  if (ready === null) return <Busy />;
  if (!ready)
    return (
      <Auth
        signingOut={signingOut}
        signOutError={signOutError}
        onReady={acceptSession}
      />
    );
  const flowId = new URLSearchParams(location.search).get("connection_flow");
  const connectionError = new URLSearchParams(location.search).get(
    "connection_error",
  );
  const matching = (value: string) =>
    value.toLowerCase().includes(search.toLowerCase());
  const activePubs = publications.filter(
    (p) => !["published", "fixture_complete", "canceled"].includes(p.state),
  );
  return (
    <ApiFeedbackContext.Provider value={error}>
      <div className="app-shell">
        <a className="skip-link" href="#main-content">
          Skip to content
        </a>
        {sidebar && (
          <button
            className="sidebar-backdrop"
            aria-label="Close navigation"
            onClick={() => setSidebar(false)}
          />
        )}
        <WorkspaceSidebar workspace={workspace} />
        <div className="main-shell">
          <header className="topbar">
            <Button
              variant="ghost"
              size="icon"
              className="mobile-menu"
              aria-label="Open navigation"
              onClick={() => setSidebar(true)}
            >
              <Menu size={22} />
            </Button>
            <div className="breadcrumb">
              Workspace <ChevronRight size={14} />
              <strong>{brand?.name ?? "Getting started"}</strong>
            </div>
            <div className="topbar-actions">
              <span className="timezone">
                <Clock3 size={14} />
                {zone.replaceAll("_", " ")}
              </span>
              <Button
                variant="ghost"
                size="icon"
                onClick={() => void load()}
                aria-label="Refresh workspace"
              >
                <RefreshCw size={17} className={loading ? "spin" : ""} />
              </Button>
              <span className="owner-avatar">You</span>
            </div>
          </header>
          <main id="main-content" tabIndex={-1}>
            <header className="page-heading">
              <div>
                <div className="eyebrow">
                  {route === "queue"
                    ? "LET’S MAKE SOMETHING GOOD"
                    : (brand?.name ?? "HELLO, CREATOR")}
                </div>
                <h1>
                  {titles[route][0]}
                  {route === "ideas" && (
                    <span className="heading-spark">✦</span>
                  )}
                </h1>
                <p>{titles[route][1]}</p>
              </div>
              {brand && ["queue", "calendar", "drafts"].includes(route) && (
                <Button onClick={() => setComposer(null)}>
                  <Plus size={18} />
                  Create post
                </Button>
              )}
            </header>
            {error && <ErrorNotice message={error} />}
            <div className="content-body" key={brandId}>
              {loading && !brand ? (
                <Busy />
              ) : !brand ? (
                <Empty
                  title="A fresh start for your ideas"
                  action={
                    <Button onClick={() => setBrandOpen(true)}>
                      <Plus size={17} />
                      Create your first brand
                    </Button>
                  }
                >
                  A brand keeps its channels, drafts, media, and publishing
                  schedule together. Start with one; add more whenever you need.
                </Empty>
              ) : (
                <>
                  {["queue", "drafts", "ideas", "media"].includes(route) && (
                    <div className="search-row">
                      <label className="search-box">
                        <Search size={17} />
                        <input
                          aria-label={`Search ${route}`}
                          placeholder={`Search ${route === "queue" ? "posts" : route}…`}
                          value={search}
                          onChange={(e) => setSearch(e.target.value)}
                        />
                        {search && (
                          <button
                            aria-label="Clear search"
                            onClick={() => setSearch("")}
                          >
                            <X size={15} />
                          </button>
                        )}
                      </label>
                    </div>
                  )}
                  {route === "queue" && (
                    <Queue
                      allPublications={publications}
                      publications={publications.filter((p) =>
                        matching(p.payload.text + " " + p.payload.title),
                      )}
                      channels={channels}
                      media={media}
                      action={action}
                      pending={pending}
                      onCompose={() => setComposer(null)}
                    />
                  )}
                  {route === "calendar" && (
                    <Calendar
                      publications={publications}
                      onCompose={(date) => setComposer({ composeDate: date })}
                    />
                  )}
                  {route === "ideas" && (
                    <Ideas
                      brand={brand}
                      ideas={ideas.filter((i) =>
                        matching(i.title + " " + i.text),
                      )}
                      action={action}
                      pending={pending}
                      onCompose={(idea) =>
                        setComposer({ title: idea.title, text: idea.text })
                      }
                    />
                  )}
                  {route === "drafts" && (
                    <Drafts
                      posts={posts.filter((p) =>
                        matching(p.title + " " + p.text),
                      )}
                      action={action}
                      pending={pending}
                      onEdit={setComposer}
                      onDuplicate={(post) =>
                        setComposer({
                          ...post,
                          id: undefined,
                          version: undefined,
                          title: post.title ? `${post.title} (copy)` : "",
                        })
                      }
                      onCompose={() => setComposer(null)}
                    />
                  )}
                  {route === "media" && (
                    <MediaLibrary
                      brand={brand}
                      media={media.filter((m) => matching(m.name))}
                      action={action}
                      pending={pending}
                    />
                  )}
                  {route === "connections" && (
                    <Connections
                      brand={brand}
                      channels={channels}
                      providers={providers}
                      action={action}
                      pending={pending}
                    />
                  )}
                  {route === "settings" && (
                    <Settings
                      channels={channels}
                      theme={theme}
                      setTheme={setTheme}
                      action={action}
                      pending={pending}
                      health={health}
                    />
                  )}
                  {route === "analytics" && (
                    <Insights
                      brand={brand}
                      channels={channels}
                      kind="analytics"
                    />
                  )}
                  {route === "inbox" && (
                    <Insights brand={brand} channels={channels} kind="inbox" />
                  )}
                </>
              )}
            </div>
            <footer className="page-footer">
              <span>Made for your own kind of magic.</span>
              <Unicorn size={20} />
            </footer>
          </main>
        </div>
        {flowId && (
          <ConnectionFlow
            flowId={flowId}
            onDone={async () => {
              go("connections");
              await load();
            }}
          />
        )}
        {connectionError && (
          <div className="toast" role="alert">
            Connection was not completed:{" "}
            {connectionError === "consent_declined"
              ? "Consent was declined. Reconnect whenever you are ready."
              : "Please reconnect from Connections."}
            <Button
              variant="ghost"
              onClick={() => {
                history.replaceState(
                  null,
                  "",
                  location.pathname + "#connections",
                );
                go("connections");
              }}
              aria-label="Dismiss connection error"
            >
              <X size={16} />
            </Button>
          </div>
        )}
        {toast && (
          <div className="toast" role="status">
            <Check size={18} />
            {toast}
          </div>
        )}
        {brandOpen && (
          <BrandDialog
            busy={pending}
            onClose={() => setBrandOpen(false)}
            onSave={createBrand}
          />
        )}{" "}
        {composer !== undefined && brand && (
          <Composer
            key={composer?.id ?? "new"}
            brand={brand}
            channels={channels}
            media={media}
            initial={composer}
            initialDate={composer?.composeDate}
            onClose={() => setComposer(undefined)}
            onSaved={load}
          />
        )}
      </div>
    </ApiFeedbackContext.Provider>
  );
}
type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;
