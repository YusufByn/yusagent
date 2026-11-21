import { useEffect, useRef, type ReactNode, type RefObject } from "react";

/**
 * A menu (the composer's pickers, the history); closes on Escape or a press
 * outside it and its trigger, the element with `data-menu-trigger={trigger}`.
 */
export function Popover({
  className,
  trigger,
  onClose,
  children,
}: {
  className: string;
  trigger: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useDismiss(ref, trigger, onClose);
  return (
    <div className={className} ref={ref}>
      {children}
    </div>
  );
}

function useDismiss(ref: RefObject<HTMLElement | null>, trigger: string, onClose: () => void) {
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    const onPointer = (event: PointerEvent) => {
      const target = event.target as Element | null;
      if (ref.current?.contains(target)) return;
      if (target?.closest(`[data-menu-trigger="${trigger}"]`)) return;
      close.current();
    };
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") close.current();
    };
    document.addEventListener("pointerdown", onPointer);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onPointer);
      document.removeEventListener("keydown", onKey);
    };
  }, [ref, trigger]);
}
