import { useState } from "react";
import { Field, Submit } from "./components/shared";
import { Button } from "./components/ui/button";
import { Dialog } from "./components/ui/dialog";
export function BrandDialog({
  busy,
  onClose,
  onSave,
}: {
  busy: boolean;
  onClose: () => void;
  onSave: (name: string) => Promise<void>;
}) {
  const [name, setName] = useState("");
  return (
    <Dialog
      open
      onOpenChange={(v) => !v && onClose()}
      title="Create a brand"
      description="Give your channels and ideas a place of their own."
    >
      <form
        className="form-stack"
        onSubmit={(e) => {
          e.preventDefault();
          void onSave(name.trim());
        }}
      >
        <Field label="Brand name">
          <input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
            maxLength={120}
            placeholder="Your brand or project"
          />
        </Field>
        <div className="dialog-footer">
          <Button variant="ghost" type="button" onClick={onClose}>
            Cancel
          </Button>
          <Submit pending={busy}>Create brand</Submit>
        </div>
      </form>
    </Dialog>
  );
}
