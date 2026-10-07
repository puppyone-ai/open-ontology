import { Dialog } from "radix-ui";
import type { ReactNode } from "react";

export type DialogFrameProps = {
  open: boolean;
  title: string;
  description: string;
  onOpenChange: (open: boolean) => void;
  children: ReactNode;
};

export function DialogFrame({ open, title, description, onOpenChange, children }: DialogFrameProps) {
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="oo-dialog-overlay" />
        <Dialog.Content className="oo-dialog" aria-describedby={undefined}>
          <Dialog.Title className="oo-section-title">{title}</Dialog.Title>
          <Dialog.Description className="oo-body">{description}</Dialog.Description>
          {children}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
