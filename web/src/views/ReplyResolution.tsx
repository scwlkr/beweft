import { useState } from "react";
import { Field } from "../components/shared";
import { Button } from "../components/ui/button";
export function ReplyResolution({
  disabled,
  onResolve,
}: {
  disabled: boolean;
  onResolve: (body: {
    resolution: "published" | "not_published";
    note: string;
    external_url?: string;
    sender_stopped?: true;
  }) => Promise<void>;
}) {
  const [resolution, setResolution] = useState("");
  const [note, setNote] = useState("");
  const [url, setUrl] = useState("");
  const [checked, setChecked] = useState(false);
  const [senderStopped, setSenderStopped] = useState(false);
  return (
    <fieldset className="form-stack detail-section" disabled={disabled}>
      <legend>Resolve the uncertain reply</legend>
      <p className="muted">
        Inspect the network first. This records your attestation; it neither
        checks the provider nor sends another reply.
      </p>
      <Field label="Verified reply outcome">
        <select
          value={resolution}
          onChange={(event) => {
            setResolution(event.target.value);
            setChecked(false);
            setSenderStopped(false);
          }}
        >
          <option value="">Choose the outcome you verified</option>
          <option value="published">The reply exists on the network</option>
          <option value="not_published">
            I verified the reply was not published
          </option>
        </select>
      </Field>
      <Field label="Reply verification note">
        <textarea
          rows={2}
          value={note}
          onChange={(event) => setNote(event.target.value)}
        />
      </Field>
      <Field label="Reply URL (optional)">
        <input
          type="url"
          value={url}
          onChange={(event) => setUrl(event.target.value)}
        />
      </Field>
      <label className="checkbox-label">
        <input
          type="checkbox"
          checked={checked}
          onChange={(event) => setChecked(event.target.checked)}
        />
        I checked the network and verified this outcome.
      </label>
      {resolution === "not_published" && (
        <label className="checkbox-label">
          <input
            type="checkbox"
            checked={senderStopped}
            onChange={(event) => setSenderStopped(event.target.checked)}
          />
          I confirmed the original sender has stopped. A lost connection or
          elapsed deadline alone does not prove this.
        </label>
      )}
      <Button
        type="button"
        variant="secondary"
        disabled={
          disabled ||
          !checked ||
          !note.trim() ||
          !resolution ||
          (resolution === "not_published" && !senderStopped)
        }
        onClick={() =>
          void onResolve({
            resolution: resolution as "published" | "not_published",
            note,
            ...(resolution === "not_published"
              ? { sender_stopped: true as const }
              : {}),
            ...(url ? { external_url: url } : {}),
          })
        }
      >
        Record reply attestation
      </Button>
    </fieldset>
  );
}
