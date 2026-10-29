import * as DialogPrimitive from "@radix-ui/react-dialog";
import { X } from "lucide-react";
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
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  title: string;
  description?: string;
  children: ReactNode;
  wide?: boolean;
}) {
  const apiError = useContext(ApiFeedbackContext);
  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="dialog-overlay" />
        <DialogPrimitive.Content
          className={`dialog-content ${wide ? "dialog-wide" : ""}`}
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
            <DialogPrimitive.Close
              className="button button-ghost button-icon"
              aria-label="Close dialog"
            >
              <X size={19} />
            </DialogPrimitive.Close>
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
