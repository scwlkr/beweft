import * as DialogPrimitive from "@radix-ui/react-dialog";
import { Maximize2, Minimize2, X } from "lucide-react";
import type { ReactNode } from "react";
import { useContext } from "react";
import { ApiFeedbackContext } from "../feedback";
import { ErrorNotice } from "../shared";
export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  wide = false,
  fullscreen = false,
  onToggleFullscreen,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  title: string;
  description?: string;
  children: ReactNode;
  wide?: boolean;
  fullscreen?: boolean;
  onToggleFullscreen?: () => void;
}) {
  const apiError = useContext(ApiFeedbackContext);
  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="dialog-overlay" />
        <DialogPrimitive.Content
          className={`dialog-content ${wide ? "dialog-wide" : ""} ${fullscreen ? "dialog-fullscreen" : ""}`}
          aria-describedby={description ? "dialog-description" : undefined}
        >
          <header className="dialog-heading">
            <div>
              <DialogPrimitive.Title>{title}</DialogPrimitive.Title>
              {description && (
                <DialogPrimitive.Description id="dialog-description">
                  {description}
                </DialogPrimitive.Description>
              )}
            </div>
            <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
              {onToggleFullscreen && (
                <button
                  type="button"
                  className="button button-ghost button-icon"
                  aria-label={fullscreen ? "Exit fullscreen" : "Fullscreen compose"}
                  aria-pressed={fullscreen}
                  onClick={onToggleFullscreen}
                >
                  {fullscreen ? <Minimize2 size={19} /> : <Maximize2 size={19} />}
                </button>
              )}
              <DialogPrimitive.Close
                className="button button-ghost button-icon"
                aria-label="Close dialog"
              >
                <X size={19} />
              </DialogPrimitive.Close>
            </div>
          </header>
          {apiError && (
            <div className="dialog-api-error">
              <ErrorNotice message={apiError} />
            </div>
          )}
          {children}
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}
