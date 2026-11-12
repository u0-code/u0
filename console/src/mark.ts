// A ghost's seal, drawn.
//
// Her seal is the hash of her birth certificate. This turns it into a mark
// in the manner of a carved name seal: strokes of one weight on a square
// lattice, mirrored left to right, filling the block. The same hash always
// gives the same mark and a different certificate gives a different one, so
// the mark is hers in the way a face is, and it cannot be chosen.

/** A stroke between two lattice points, each coordinate 0 to 6. */
export type Stroke = [x1: number, y1: number, x2: number, y2: number];

const HALF = 4; // columns 0..3; column 3 is the axis of symmetry
const ROWS = 7;
const SPAN = 6; // the lattice runs 0..6 both ways

/** A small, good generator seeded from the seal (sfc32). */
function generator(seal: string): () => number {
  const words = [0x9e3779b9, 0x243f6a88, 0xb7e15162, 0xdeadbeef];
  for (let i = 0; i < seal.length; i++) {
    const slot = i % 4;
    words[slot] = (Math.imul(words[slot]! ^ seal.charCodeAt(i), 0x01000193) + i) >>> 0;
  }
  let [a, b, c, d] = words as [number, number, number, number];
  const next = () => {
    a >>>= 0;
    b >>>= 0;
    c >>>= 0;
    d >>>= 0;
    const t = (((a + b) | 0) + d) | 0;
    d = (d + 1) | 0;
    a = b ^ (b >>> 9);
    b = (c + (c << 3)) | 0;
    c = (c << 21) | (c >>> 11);
    c = (c + t) | 0;
    return (t >>> 0) / 4294967296;
  };
  for (let i = 0; i < 16; i++) next();
  return next;
}

const key = (x: number, y: number) => y * HALF + x;

/** The strokes of the mark for `seal`. */
export function strokes(seal: string): Stroke[] {
  const random = generator(seal);
  const visited = new Set<number>();
  const half: Stroke[] = [];

  // Walk the left half of the lattice, preferring to keep going straight:
  // long strokes with few turns read as writing, short ones as noise.
  type Step = { x: number; y: number; dx: number; dy: number };
  const start: Step = { x: HALF - 1, y: Math.floor(random() * ROWS), dx: 0, dy: 0 };
  const stack: Step[] = [start];
  visited.add(key(start.x, start.y));
  while (stack.length > 0) {
    const here = stack[stack.length - 1]!;
    const ways = [
      [1, 0],
      [-1, 0],
      [0, 1],
      [0, -1],
    ]
      .map(([dx, dy]) => ({ x: here.x + dx!, y: here.y + dy!, dx: dx!, dy: dy! }))
      .filter((n) => n.x >= 0 && n.x < HALF && n.y >= 0 && n.y < ROWS && !visited.has(key(n.x, n.y)));
    if (ways.length === 0) {
      stack.pop();
      continue;
    }
    const straight = ways.find((n) => n.dx === here.dx && n.dy === here.dy);
    const next = straight && random() < 0.68 ? straight : ways[Math.floor(random() * ways.length)]!;
    visited.add(key(next.x, next.y));
    half.push([here.x, here.y, next.x, next.y]);
    stack.push(next);
  }

  // Trim some stroke ends so the block breathes.
  for (let pass = 0; pass < 2; pass++) {
    const degree = new Map<number, number>();
    for (const [x1, y1, x2, y2] of half) {
      degree.set(key(x1, y1), (degree.get(key(x1, y1)) ?? 0) + 1);
      degree.set(key(x2, y2), (degree.get(key(x2, y2)) ?? 0) + 1);
    }
    for (let i = half.length - 1; i >= 0 && half.length > 17; i--) {
      const [x1, y1, x2, y2] = half[i]!;
      const loose = degree.get(key(x1, y1)) === 1 || degree.get(key(x2, y2)) === 1;
      // Ends on the axis stay: they are what joins the two halves.
      const onAxis = x1 === HALF - 1 && x2 === HALF - 1;
      if (loose && !onAxis && random() < 0.42) half.splice(i, 1);
    }
  }

  const all: Stroke[] = [];
  for (const [x1, y1, x2, y2] of half) {
    all.push([x1, y1, x2, y2]);
    if (x1 !== SPAN - x1 || x2 !== SPAN - x2) all.push([SPAN - x1, y1, SPAN - x2, y2]);
  }
  return all;
}
