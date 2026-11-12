// A ghost's face in the window.
//
// If she has been given a picture, that. Otherwise her seal mark, on a tile
// tinted with a hue that also comes from her seal, so that two ghosts with
// no pictures are still told apart at a glance.

import { useMemo } from "react";
import { strokes } from "./mark";
import type { Presence } from "./words";

/** The hue that belongs to a seal. */
export function hue(seal: string): number {
  let hash = 0;
  for (let i = 0; i < Math.min(seal.length, 16); i++) hash = (hash * 31 + seal.charCodeAt(i)) >>> 0;
  return hash % 360;
}

const at = (n: number) => (13 + (n * 38) / 6).toFixed(1);

/** Her seal mark as a single path on a 64 by 64 field. */
export function Mark({ seal }: { seal: string }) {
  const d = useMemo(
    () =>
      strokes(seal)
        .map(([x1, y1, x2, y2]) => `M${at(x1)} ${at(y1)}L${at(x2)} ${at(y2)}`)
        .join(""),
    [seal],
  );
  return (
    <svg viewBox="0 0 64 64" aria-hidden="true">
      <path d={d} fill="none" stroke="currentColor" strokeWidth="3.9" strokeLinecap="square" />
    </svg>
  );
}

type Size = "xs" | "sm" | "md" | "lg" | "xl" | "xxl";

export function Avatar({
  seal,
  picture,
  size = "md",
  state,
}: {
  seal: string;
  picture?: string | null;
  size?: Size;
  /** Shown as a small badge. Left out, there is no badge. */
  state?: Presence;
}) {
  return (
    <span className={`av ${size}${picture ? " pic" : ""}`} style={{ "--h": hue(seal) } as React.CSSProperties}>
      {picture ? <img src={picture} alt="" draggable={false} /> : seal ? <Mark seal={seal} /> : null}
      {state === "ok" && <i className="st ok" />}
      {state === "busy" && <i className="st busy" />}
      {state === "wait" && <i className="st wait" />}
      {state === "asleep" && (
        <i className="st sleep">
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z" />
          </svg>
        </i>
      )}
    </span>
  );
}
