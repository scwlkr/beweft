import { AlertCircle, ArrowUpRight, LoaderCircle } from "lucide-react";
import type { ReactNode } from "react";
import { Unicorn } from "./brand";
import { Button } from "./ui/button";
export function Empty({
  title,
  children,
  action,
}: {
  title: string;
  children: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="empty-state">
      <div className="empty-mark">
        <Unicorn size={54} />
      </div>
      <h2>{title}</h2>
      <p>{children}</p>
      {action}
    </div>
  );
}
export function ErrorNotice({ message }: { message: string }) {
  return (
    <div className="notice notice-error" role="alert">
      <AlertCircle size={18} />
      <span>{message}</span>
    </div>
  );
}
export function Busy() {
  return (
    <div className="loading" role="status">
      <LoaderCircle className="spin" size={22} /> Loading your workspace…
    </div>
  );
}
export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <label className="field">
      <span>{label}</span>
      {children}
      {hint && <small>{hint}</small>}
    </label>
  );
}
export function ExternalLink({
  url,
  children,
}: {
  url: string;
  children: ReactNode;
}) {
  let safe = false;
  try {
    safe = ["https:", "http:"].includes(new URL(url).protocol);
  } catch {
    /* invalid URLs are not links */
  }
  return safe ? (
    <a href={url} target="_blank" rel="noreferrer" className="text-link">
      {children}
      <ArrowUpRight size={14} />
    </a>
  ) : (
    <span>{children}</span>
  );
}
export function Time({ value }: { value: string }) {
  return (
    <time dateTime={value} title={new Date(value).toLocaleString()}>
      {new Date(value).toLocaleString(undefined, {
        month: "short",
        day: "numeric",
        hour: "numeric",
        minute: "2-digit",
      })}
    </time>
  );
}
export function State({ state }: { state: string }) {
  const labels: Record<string, string> = {
    manual_required: "Ready for handoff",
    uncertain: "Needs reconciliation",
    fixture_complete: "Fixture complete",
    sending: "Publishing",
    published: "Published",
    queued: "Scheduled",
    failed: "Failed",
    canceled: "Canceled",
  };
  return (
    <span className={`status status-${state}`}>{labels[state] ?? state}</span>
  );
}
export function Submit({
  pending,
  children,
}: {
  pending: boolean;
  children: ReactNode;
}) {
  return (
    <Button type="submit" disabled={pending}>
      {pending && <LoaderCircle size={16} className="spin" />}
      {children}
    </Button>
  );
}
export const localDateTime = (value?: string, includeMilliseconds = false) => {
  const d = value ? new Date(value) : new Date(Date.now() + 3600000);
  return new Date(d.getTime() - d.getTimezoneOffset() * 60000)
    .toISOString()
    .slice(0, includeMilliseconds ? 23 : 16);
};
