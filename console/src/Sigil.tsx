import { useMemo } from "react";
import { strokes } from "./mark";

const BOX = 64;
const EDGE = 2.5;
const INSET = 12.5;
const STEP = (BOX - 2 * INSET) / 6;

/**
 * Her seal as a mark. `carved` is the block with the strokes cut out of it,
 * for small sizes; `inked` is the strokes alone inside a ruled border, the
 * way a seal prints, for where there is room.
 */
export function Sigil({
  seal,
  size = 28,
  inked = false,
  title,
}: {
  seal: string;
  size?: number;
  inked?: boolean;
  title?: string;
}) {
  const path = useMemo(
    () =>
      strokes(seal)
        .map(
          ([x1, y1, x2, y2]) =>
            `M${(INSET + x1 * STEP).toFixed(2)} ${(INSET + y1 * STEP).toFixed(2)}L${(INSET + x2 * STEP).toFixed(2)} ${(INSET + y2 * STEP).toFixed(2)}`,
        )
        .join(""),
    [seal],
  );
  return (
    <svg
      className={`sigil${inked ? " is-inked" : ""}`}
      width={size}
      height={size}
      viewBox={`0 0 ${BOX} ${BOX}`}
      role={title ? "img" : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
    >
      {inked ? (
        <rect className="sigil-rule" x={EDGE + 1.6} y={EDGE + 1.6} width={BOX - 2 * EDGE - 3.2} height={BOX - 2 * EDGE - 3.2} rx="5" />
      ) : (
        <rect className="sigil-block" x={EDGE} y={EDGE} width={BOX - 2 * EDGE} height={BOX - 2 * EDGE} rx="6.5" />
      )}
      <path className="sigil-strokes" d={path} />
    </svg>
  );
}
