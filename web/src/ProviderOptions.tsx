import { Field } from "./components/shared";
export function ProviderOptions({
  provider,
  value,
  onChange,
}: {
  provider: string;
  value: Record<string, unknown>;
  onChange: (v: Record<string, unknown>) => void;
}) {
  const set = (key: string, v: unknown) => onChange({ ...value, [key]: v });
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
