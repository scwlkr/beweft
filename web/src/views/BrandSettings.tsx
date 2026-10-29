import { useCallback, useEffect, useRef, useState } from "react";
import { z } from "zod";
import {
  api,
  BrandSchema,
  captureWorkspaceSession,
  isCurrentWorkspaceSession,
  type Brand,
} from "../api";
import { ErrorNotice, Field, Submit } from "../components/shared";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";
type Action = (
  fn: () => Promise<unknown>,
  message?: string,
) => Promise<boolean>;
export function BrandSettings({
  action,
  pending,
}: {
  action: Action;
  pending: boolean;
}) {
  const [brands, setBrands] = useState<Brand[]>([]);
  const [error, setError] = useState("");
  const [editing, setEditing] = useState<Brand | null>(null);
  const [name, setName] = useState("");
  const [confirm, setConfirm] = useState<Brand | null>(null);
  const alive = useRef(false);
  const sequence = useRef(0);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      ++sequence.current;
    };
  }, []);
  const load = useCallback(async () => {
    let session;
    try {
      session = captureWorkspaceSession();
    } catch {
      return;
    }
    const id = ++sequence.current;
    try {
      const list = await api(
        "/brands?include_archived=true",
        z.array(BrandSchema),
      );
      if (
        alive.current &&
        id === sequence.current &&
        isCurrentWorkspaceSession(session)
      ) {
        setBrands(list);
        setError("");
      }
    } catch (failure) {
      if (
        alive.current &&
        id === sequence.current &&
        isCurrentWorkspaceSession(session)
      )
        setError((failure as Error).message);
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load]);
  return (
    <section className="settings-section form-stack">
      <h2>Brands</h2>
      <p className="muted">
        Archiving preserves content and pauses channels. A brand with a delivery
        in progress cannot be archived.
      </p>
      {error && (
        <>
          <ErrorNotice message={error} />
          <Button variant="secondary" onClick={() => void load()}>
            Reload brands
          </Button>
        </>
      )}
      {brands.map((brand) => (
        <div className="token-row" key={brand.id}>
          <div>
            <strong>{brand.name}</strong>
            <small>
              {brand.archived_at
                ? "Archived · channels remain paused after restore"
                : "Active"}
            </small>
          </div>
          <Button
            variant="secondary"
            size="sm"
            onClick={() => {
              setEditing(brand);
              setName(brand.name);
            }}
          >
            Rename
          </Button>
          <Button
            variant="ghost"
            size="sm"
            disabled={pending}
            onClick={() => setConfirm(brand)}
          >
            {brand.archived_at ? "Restore" : "Archive"}
          </Button>
        </div>
      ))}
      {editing && (
        <Dialog
          open
          onOpenChange={(open) => !open && setEditing(null)}
          title="Rename brand"
        >
          <form
            className="form-stack"
            onSubmit={(event) => {
              event.preventDefault();
              void action(
                () =>
                  api(`/brands/${editing.id}`, BrandSchema, "PATCH", { name }),
                "Brand renamed",
              ).then(async (done) => {
                if (!alive.current) return;
                if (done) setEditing(null);
                await load();
              });
            }}
          >
            <Field label="Brand name">
              <input
                required
                maxLength={100}
                value={name}
                onChange={(event) => setName(event.target.value)}
              />
            </Field>
            <Submit pending={pending}>Save name</Submit>
          </form>
        </Dialog>
      )}
      {confirm && (
        <Dialog
          open
          onOpenChange={(open) => !open && setConfirm(null)}
          title={confirm.archived_at ? "Restore brand?" : "Archive brand?"}
          description={
            confirm.archived_at
              ? "Channels stay paused after restoring. Resume each channel when you are ready."
              : "Existing content is preserved and all channels are paused. Deliveries in progress must finish first."
          }
        >
          <div className="dialog-footer">
            <Button variant="secondary" onClick={() => setConfirm(null)}>
              Cancel
            </Button>
            <Button
              disabled={pending}
              variant={confirm.archived_at ? "default" : "destructive"}
              onClick={() =>
                void action(
                  () =>
                    api(`/brands/${confirm.id}`, BrandSchema, "PATCH", {
                      archived: !confirm.archived_at,
                    }),
                  confirm.archived_at
                    ? "Brand restored; channels remain paused"
                    : "Brand archived",
                ).then(async (done) => {
                  if (!alive.current) return;
                  if (done) setConfirm(null);
                  await load();
                })
              }
            >
              {confirm.archived_at ? "Restore brand" : "Archive brand"}
            </Button>
          </div>
        </Dialog>
      )}
    </section>
  );
}
