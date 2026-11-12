// A small popover that belongs to the control that opened it, and the clock
// the window reads.

import { useEffect, useRef, useState, type ReactNode } from "react";

/** The current time, refreshed every `every` milliseconds. */
export function useNow(every = 1000): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), every);
    return () => clearInterval(timer);
  }, [every]);
  return now;
}

/**
 * A control and the menu it opens. The menu closes on a click anywhere else,
 * on Escape, and when something in it is chosen.
 */
export function Popover({
  button,
  children,
  place = "below",
  align = "start",
  className = "",
}: {
  button: (open: boolean, toggle: () => void) => ReactNode;
  children: (close: () => void) => ReactNode;
  place?: "above" | "below";
  align?: "start" | "end";
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    if (!open) return;
    const away = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("pointerdown", away);
    window.addEventListener("keydown", escape);
    return () => {
      window.removeEventListener("pointerdown", away);
      window.removeEventListener("keydown", escape);
    };
  }, [open]);

  return (
    <span className="pop" ref={root}>
      {button(open, () => setOpen((was) => !was))}
      {open && (
        <div className={`menu ${place} ${align} ${className}`} role="menu">
          {children(() => setOpen(false))}
        </div>
      )}
    </span>
  );
}
