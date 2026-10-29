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
import { useEffect, useState } from "react";
import { Auth } from "./Auth";
import { BrandDialog } from "./BrandDialog";
import { Unicorn } from "./components/brand";
import { ApiFeedbackContext } from "./components/feedback";
import { Busy, Empty, ErrorNotice } from "./components/shared";
import { Button } from "./components/ui/button";
import { Composer } from "./Composer";
import { titles, zone } from "./navigation";
import { useWorkspace } from "./useWorkspace";
import { usePendingSchedule } from "./usePendingSchedule";
import { isCurrentReplacement } from "./replacementIntent";
import { usePendingReplacement } from "./usePendingReplacement";
import { usePendingMedia } from "./usePendingMedia";
import { Calendar } from "./views/Calendar";
import { ConnectionFlow } from "./views/ConnectionFlow";
import { Connections } from "./views/Connections";
import { Drafts } from "./views/Drafts";
import { Ideas } from "./views/Ideas";
import { Insights } from "./views/Insights";
import { MediaDetails, MediaLibrary } from "./views/MediaLibrary";
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
    sessionCheckFailed,
    retrySession,
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
    reconcileMedia,
    go,
    createBrand,
    endSessionWith,
  } = workspace;
  const pendingSchedule = usePendingSchedule(brandId, ready === true);
  const pendingReplacement = usePendingReplacement(
    pendingSchedule.key,
    brandId,
  );
  const pendingMedia = usePendingMedia(ready ? brandId : "", reconcileMedia);
  const [reviewMediaId, setReviewMediaId] = useState<string | null>(null);
  const [confirmReplacementReview, setConfirmReplacementReview] =
    useState(false);
  useEffect(
    () => setConfirmReplacementReview(false),
    [pendingReplacement.review],
  );
  useEffect(() => setReviewMediaId(null), [brandId]);
  const reviewMedia = media.find((item) => item.id === reviewMediaId);
  if (ready === null) return <Busy />;
  if (!ready)
    return (
      <Auth
        signingOut={signingOut}
        signOutError={signOutError}
        sessionCheckFailed={sessionCheckFailed}
        onRetrySession={retrySession}
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
            {pendingSchedule.intent && (
              <div className="notice" role="status">
                <span>
                  A schedule needs review before another post can be saved for
                  this brand.
                </span>
                <Button
                  variant="secondary"
                  onClick={() => setComposer(pendingSchedule.intent!.post)}
                >
                  Review pending schedule
                </Button>
              </div>
            )}
            {(pendingReplacement.intent || pendingReplacement.error) && (
              <div className="notice" role="status">
                <div>
                  <p>
                    {pendingReplacement.intent
                      ? isCurrentReplacement(pendingReplacement.intent) &&
                        pendingReplacement.intent.rejection
                        ? "The server rejected this replacement. Review the original before starting another publication."
                        : "A queued replacement has an unconfirmed outcome. Check it before starting another publication."
                      : pendingReplacement.error}
                  </p>
                  {pendingReplacement.intent && (
                    <p>
                      Original:{" "}
                      {publications.find(
                        (p) =>
                          p.id === pendingReplacement.intent?.publicationId,
                      )?.payload.title || "Untitled publication"}{" "}
                      · <code>{pendingReplacement.intent.publicationId}</code>.
                      Check its outcome before taking another action. If it is
                      still queued, you can cancel the original and check again.
                    </p>
                  )}
                  {pendingReplacement.intent && pendingReplacement.error && (
                    <p>{pendingReplacement.error}</p>
                  )}
                  {pendingReplacement.review && (
                    <div className="form-stack">
                      <p>
                        Original:{" "}
                        {pendingReplacement.review.status.original.state.replaceAll(
                          "_",
                          " ",
                        )}{" "}
                        on{" "}
                        {pendingReplacement.review.status.original.channel_name}{" "}
                        · revision{" "}
                        {pendingReplacement.review.status.original.post_version}
                        .{" "}
                        {pendingReplacement.review.rejected
                          ? "The server rejected this replacement request; the original may still publish or be retried."
                          : "The original reached a terminal state."}
                      </p>
                      <p>
                        {pendingReplacement.review.status.replacement ? (
                          <>
                            Linked replacement:{" "}
                            <code>
                              {pendingReplacement.review.status.replacement.id}
                            </code>{" "}
                            · revision{" "}
                            {
                              pendingReplacement.review.status.replacement
                                .post_version
                            }{" "}
                            ·{" "}
                            {pendingReplacement.review.status.replacement.state.replaceAll(
                              "_",
                              " ",
                            )}
                            . Inspect it in Publishing before deciding.
                          </>
                        ) : (
                          "No replacement is linked to this original."
                        )}
                      </p>
                      <label className="checkbox-label">
                        <input
                          type="checkbox"
                          checked={confirmReplacementReview}
                          onChange={(event) =>
                            setConfirmReplacementReview(event.target.checked)
                          }
                        />
                        I reviewed these outcomes and understand that any new
                        schedule is a separate action.
                      </label>
                      <Button
                        variant="secondary"
                        disabled={
                          !confirmReplacementReview || pendingReplacement.busy
                        }
                        onClick={() =>
                          void pendingReplacement.resolve().then(() => load())
                        }
                      >
                        Finish review
                      </Button>
                    </div>
                  )}
                </div>
                {pendingReplacement.intent && (
                  <Button
                    variant="secondary"
                    disabled={pendingReplacement.busy}
                    onClick={() =>
                      void pendingReplacement.check().then(() => load())
                    }
                  >
                    Check replacement
                  </Button>
                )}
              </div>
            )}
            {brand &&
              pendingMedia.brand === brand.id &&
              Object.keys(pendingMedia.intents).length > 0 && (
                <div className="quiet-note" role="status">
                  <p>
                    Some media descriptions are unconfirmed. Review them before
                    scheduling posts that use them.
                  </p>
                  {Object.keys(pendingMedia.intents).map((id) => {
                    const item = media.find((row) => row.id === id);
                    return item ? (
                      <Button
                        key={id}
                        variant="secondary"
                        onClick={() => setReviewMediaId(id)}
                      >
                        Review {item.name}
                      </Button>
                    ) : null;
                  })}
                </div>
              )}
            {brand && pendingMedia.error && (
              <div className="quiet-note" role="status">
                <p>{pendingMedia.error}</p>
                <Button variant="secondary" onClick={pendingMedia.retry}>
                  Check media again
                </Button>
              </div>
            )}
            {brand && Object.keys(pendingMedia.assetErrors).length > 0 && (
              <div className="quiet-note" role="status">
                <p>
                  Some media descriptions could not be checked. Posts using
                  those assets remain blocked.
                </p>
                <Button variant="secondary" onClick={pendingMedia.retry}>
                  Check media again
                </Button>
              </div>
            )}
            <div className="content-body" key={brandId}>
              {loading && !brand ? (
                <Busy />
              ) : !brand && route !== "settings" ? (
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
                      posts={posts}
                      pendingSchedule={pendingSchedule}
                      pendingMedia={pendingMedia}
                      reconcileMedia={reconcileMedia}
                      onEdit={setComposer}
                      onRefresh={load}
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
                      brand={brand!}
                      ideas={ideas.filter((i) =>
                        matching(
                          i.title + " " + i.text + " " + i.tags.join(" "),
                        ),
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
                      brand={brand!}
                      channels={channels}
                      posts={posts.filter((p) =>
                        matching(p.title + " " + p.text),
                      )}
                      action={action}
                      pending={pending}
                      protectedPostId={pendingSchedule.intent?.post.id}
                      pendingSchedule={pendingSchedule}
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
                      brand={brand!}
                      media={media.filter((m) => matching(m.name))}
                      action={action}
                      pending={pending}
                      reconcileMedia={reconcileMedia}
                    />
                  )}
                  {route === "connections" && (
                    <Connections
                      brand={brand!}
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
                      endSessionWith={endSessionWith}
                    />
                  )}
                  {route === "analytics" && (
                    <Insights
                      brand={brand!}
                      channels={channels}
                      kind="analytics"
                    />
                  )}
                  {route === "inbox" && (
                    <Insights brand={brand!} channels={channels} kind="inbox" />
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
            brandId={brandId}
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
            pendingSchedule={pendingSchedule}
            pendingMedia={pendingMedia}
            reconcileMedia={reconcileMedia}
            onReviewMedia={setReviewMediaId}
          />
        )}
        {reviewMedia && brand && (
          <MediaDetails
            key={reviewMedia.id}
            selected={reviewMedia}
            action={action}
            pending={pending}
            limit={null}
            reconcileMedia={reconcileMedia}
            onClose={() => setReviewMediaId(null)}
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
