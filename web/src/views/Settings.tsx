import { ArrowUpRight, Check } from "lucide-react";
import { useState } from "react";
import { type Channel, type Health } from "../api";
import { Time } from "../components/shared";

import { Schedules } from "./Schedules";
import { Tokens } from "./Tokens";
type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;
export function Settings({
  channels,
  theme,
  setTheme,
  action,
  pending,
  health,
}: {
  channels: Channel[];
  theme: string;
  setTheme: (t: string) => void;
  action: Action;
  pending: boolean;
  health: Health | null;
}) {
  const [tab, setTab] = useState("schedules");
  return (
    <>
      <div className="tabs settings-tabs">
        <button
          aria-pressed={tab === "schedules"}
          onClick={() => setTab("schedules")}
        >
          Posting schedules
        </button>
        <button
          aria-pressed={tab === "preferences"}
          onClick={() => setTab("preferences")}
        >
          Appearance
        </button>
        <button
          aria-pressed={tab === "agents"}
          onClick={() => setTab("agents")}
        >
          API & agents
        </button>
        <button
          aria-pressed={tab === "system"}
          onClick={() => setTab("system")}
        >
          System
        </button>
      </div>
      {tab === "schedules" && (
        <Schedules channels={channels} action={action} pending={pending} />
      )}{" "}
      {tab === "preferences" && (
        <section className="settings-section">
          <h2>A workspace that feels like you</h2>
          <p className="muted">Choose how beweft looks on this device.</p>
          <div className="theme-options">
            {["light", "dark", "system"].map((t) => (
              <button
                className={`theme-option ${theme === t ? "selected" : ""}`}
                key={t}
                aria-pressed={theme === t}
                onClick={() => setTheme(t)}
              >
                <div className={`theme-preview theme-${t}`}>
                  <i />
                  <div>
                    <b />
                    <b />
                    <b />
                  </div>
                </div>
                <span>
                  {t[0].toUpperCase() + t.slice(1)}
                  {theme === t && <Check size={16} />}
                </span>
              </button>
            ))}
          </div>
        </section>
      )}
      {tab === "agents" && <Tokens action={action} pending={pending} />}{" "}
      {tab === "system" && (
        <section className="settings-section">
          <h2>Publishing worker</h2>
          <p className="muted">
            The server worker runs independently of this browser. Keeping your
            Mac or Codex open is not required when deployed to an always-on
            host.
          </p>
          <dl className="connection-facts">
            <div>
              <dt>Worker</dt>
              <dd>{health?.worker_healthy ? "Healthy" : "Needs attention"}</dd>
            </div>
            <div>
              <dt>Last heartbeat</dt>
              <dd>
                {health?.worker_last_seen ? (
                  <Time value={health.worker_last_seen} />
                ) : (
                  "Not received"
                )}
              </dd>
            </div>
            <div>
              <dt>Queued</dt>
              <dd>{health?.queued ?? "—"}</dd>
            </div>
            <div>
              <dt>Uncertain outcomes</dt>
              <dd>{health?.uncertain ?? "—"}</dd>
            </div>
            <div>
              <dt>Manual handoffs</dt>
              <dd>{health?.manual_required ?? "—"}</dd>
            </div>
          </dl>
          <h3>Server maintenance</h3>
          <p className="muted">
            Back up PostgreSQL and the media volume together. Keep your
            encryption key with the backup in a secure store. Use the
            self-hosting guide for upgrades and recovery.
          </p>
          <a
            className="text-link"
            href="https://github.com/scwlkr/beweft/tree/main/docs"
            target="_blank"
            rel="noreferrer"
          >
            Open self-hosting documentation <ArrowUpRight size={15} />
          </a>
        </section>
      )}
    </>
  );
}
