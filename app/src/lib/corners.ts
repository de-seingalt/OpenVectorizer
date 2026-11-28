// Per-corner editing. A corner is identified by its apex position: edits are stored in
// `options.corners.edits` and the engine finds each corner again at every tracing.
import type { CornerEdit, CornerInfo, Options } from '$lib/engine';

/** Must match `CORNER_EDIT_REACH` in crates/core/src/options.rs. */
const REACH = 1.5;

type Point = { x: number; y: number };
const dist = (a: Point, b: Point) => Math.hypot(a.x - b.x, a.y - b.y);

/** The corner nearest to `p` within reach, if any. */
export function findCorner(corners: CornerInfo[], p: Point): CornerInfo | null {
  let best: CornerInfo | null = null;
  for (const k of corners) if (dist(k, p) <= REACH && (!best || dist(k, p) < dist(best, p))) best = k;
  return best;
}

/** `corner` and, when asked, the other corners of its group. */
export function targets(corners: CornerInfo[], corner: CornerInfo, withGroup: boolean): CornerInfo[] {
  if (!withGroup || corner.group === null) return [corner];
  return corners.filter((k) => k.group === corner.group);
}

/** Replace the edits of the `of` corners by `change`, or remove them when `change` is null. */
export function editCorners(options: Options, of: CornerInfo[], change: Omit<CornerEdit, 'x' | 'y'> | null) {
  const near = (e: CornerEdit) => of.some((k) => dist(k, e) < 0.5);
  const kept = options.corners.edits.filter((e) => !near(e));
  const added = change ? of.map((k) => ({ x: k.x, y: k.y, ...change })) : [];
  options.corners = { ...options.corners, edits: [...kept, ...added] };
}
