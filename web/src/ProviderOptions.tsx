import { Field } from "./components/shared";
export function ProviderOptions({
  provider,
  capabilities,
  value,
  onChange,
}: {
  provider: string;
  capabilities: unknown;
  value: Record<string, unknown>;
  onChange: (v: Record<string, unknown>) => void;
}) {
  const set = (key: string, v: unknown) => onChange({ ...value, [key]: v });
  const caps =
    capabilities && typeof capabilities === "object"
      ? (capabilities as Record<string, unknown>)
      : {};
  const formats = Array.isArray(caps.formats) ? caps.formats : [];
  if (provider === "facebook" && formats.includes("reel")) {
    const permissions =
      caps.permissions && typeof caps.permissions === "object"
        ? (caps.permissions as Record<string, unknown>)
        : {};
    const actions =
      permissions.actions && typeof permissions.actions === "object"
        ? (permissions.actions as Record<string, unknown>)
        : {};
    const reel =
      actions.reel && typeof actions.reel === "object"
        ? (actions.reel as Record<string, unknown>)
        : {};
    return (
      <section className="provider-options form-stack">
        <Field label="Facebook post format">
          <select
            value={value.format === "reel" ? "reel" : ""}
            onChange={(event) => {
              const next = { ...value };
              if (event.target.value) next.format = "reel";
              else delete next.format;
              onChange(next);
            }}
          >
            <option value="">Text or image</option>
            <option value="reel">Reel</option>
          </select>
        </Field>
        {value.format === "reel" && (
          <p className="muted">
            Use one MP4 without a video description. Page permissions and
            account eligibility apply.{" "}
            {reel.status === "missing"
              ? "The last permission check reported missing Reel permissions."
              : reel.status !== "granted"
                ? "Reel permission status is not confirmed."
                : "Reported Reel permissions are present; account eligibility is not yet verified."}
          </p>
        )}
      </section>
    );
  }
  if (
    (provider === "instagram" || provider === "threads") &&
    formats.includes("carousel")
  )
    return (
      <section className="provider-options form-stack">
        <p className="muted">
          A carousel uses 2–10 attachments in their selected order. Use
          supported JPEG images or MP4 videos; Threads also accepts PNG images.
          Stories require manual handoff.
        </p>
        {provider === "threads" && (
          <Field label="Who can reply?">
            <select
              value={String(value.reply_control ?? "everyone")}
              onChange={(event) => set("reply_control", event.target.value)}
            >
              <option value="everyone">Everyone</option>
              <option value="accounts_you_follow">Accounts you follow</option>
              <option value="mentioned_only">Mentioned accounts</option>
            </select>
          </Field>
        )}
      </section>
    );
  if (provider === "x")
    return (
      <section className="provider-options form-stack">
        <h3>X video requirements</h3>
        <p className="muted">
          The current beweft adapter accepts one MP4 video with H.264,
          progressive yuv420p, at most 60 fps and 256 MiB. The longer side must
          be at most 1,280 pixels, the shorter side at most 1,024 pixels, and
          the aspect ratio between 1:3 and 3:1. These are this adapter’s limits.
          X video descriptions are not supported; the adapter rejects a video
          that has a description instead of dropping it.
        </p>
      </section>
    );
  if (provider === "youtube")
    return (
      <section className="provider-options form-stack">
        <h3>YouTube video settings</h3>
        <p className="muted">
          Use the post title as your video title. Your account’s API approval
          can restrict uploads to private visibility.
        </p>
        <Field label="Video privacy">
          <select
            required
            value={String(value.privacy ?? "")}
            onChange={(e) => set("privacy", e.target.value)}
          >
            <option value="">Choose privacy</option>
            <option value="private">Private</option>
            <option value="unlisted">Unlisted</option>
            <option value="public">Public</option>
          </select>
        </Field>
        <Field label="Is this video made for kids?">
          <select
            required
            value={
              value.made_for_kids === undefined
                ? ""
                : String(value.made_for_kids)
            }
            onChange={(e) => set("made_for_kids", e.target.value === "true")}
          >
            <option value="">Choose audience designation</option>
            <option value="true">Yes, made for kids</option>
            <option value="false">No, not made for kids</option>
          </select>
        </Field>
        <Field
          label="Category ID"
          hint="Optional YouTube category ID. Leave blank for the adapter default."
        >
          <input
            value={String(value.category_id ?? "")}
            onChange={(e) => set("category_id", e.target.value)}
            inputMode="numeric"
          />
        </Field>
        <label className="checkbox-label">
          <input
            type="checkbox"
            checked={value.contains_synthetic_media === true}
            onChange={(e) => set("contains_synthetic_media", e.target.checked)}
          />
          Contains altered or synthetic media
        </label>
      </section>
    );
  if (provider === "mastodon")
    return (
      <section className="provider-options form-stack">
        <h3>Mastodon settings</h3>
        <Field label="Visibility">
          <select
            value={String(value.visibility ?? "public")}
            onChange={(e) => set("visibility", e.target.value)}
          >
            <option value="public">Public</option>
            <option value="unlisted">Unlisted</option>
            <option value="private">Followers only</option>
            <option value="direct">Mentioned accounts only</option>
          </select>
        </Field>
        <Field label="Content warning">
          <input
            value={String(value.content_warning ?? "")}
            onChange={(e) => set("content_warning", e.target.value)}
          />
        </Field>
        <label className="checkbox-label">
          <input
            type="checkbox"
            checked={value.sensitive === true}
            onChange={(e) => set("sensitive", e.target.checked)}
          />
          Mark media as sensitive
        </label>
      </section>
    );
  if (provider === "threads")
    return (
      <Field label="Who can reply?">
        <select
          value={String(value.reply_control ?? "everyone")}
          onChange={(e) => set("reply_control", e.target.value)}
        >
          <option value="everyone">Everyone</option>
          <option value="accounts_you_follow">Accounts you follow</option>
          <option value="mentioned_only">Mentioned accounts</option>
        </select>
      </Field>
    );
  if (provider === "pinterest")
    return (
      <section className="provider-options form-stack">
        <Field label="Pinterest board ID">
          <input
            required
            value={String(value.board_id ?? "")}
            onChange={(e) => set("board_id", e.target.value)}
          />
        </Field>
        <Field label="Destination link">
          <input
            type="url"
            value={String(value.link ?? "")}
            onChange={(e) => set("link", e.target.value)}
          />
        </Field>
      </section>
    );
  return null;
}
