// Guides (straight lines, tangent arcs, circles, ellipses, symmetries found by the engine).
// A guide turned off is stored in `options.disabledGuides` by kind and position; the engine
// finds it again at every tracing.
import type { Guide, Options } from '$lib/engine';

/** Must match `GUIDE_REACH` in crates/core/src/guide.rs. */
const REACH = 2;

/** Turn `guide` off, or back on. */
export function toggleGuide(options: Options, guide: Guide) {
  const near = (k: { kind: string; x: number; y: number }) =>
    k.kind === guide.kind && Math.hypot(k.x - guide.x, k.y - guide.y) <= REACH;
  options.disabledGuides = guide.enabled
    ? [...options.disabledGuides, { kind: guide.kind, x: guide.x, y: guide.y }]
    : options.disabledGuides.filter((k) => !near(k));
}
