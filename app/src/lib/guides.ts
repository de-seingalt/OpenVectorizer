// Guides (straight lines, tangent arcs, circles, ellipses, symmetries found by the engine).
// The user turns followed guides off (`options.disabledGuides`) and forces candidates on
// (`options.forcedGuides`); both lists hold the kind and position, found again at every tracing.
import type { Guide, GuideKey, Options } from '$lib/engine';

/** Must match `GUIDE_REACH` in crates/core/src/guide.rs. */
const REACH = 2;

const key = (g: Guide): GuideKey => ({ kind: g.kind, x: g.x, y: g.y });
const other = (g: Guide) => (k: GuideKey) => k.kind !== g.kind || Math.hypot(k.x - g.x, k.y - g.y) > REACH;

/** Click on a guide: followed → off → followed; candidate → forced → candidate. */
export function toggleGuide(options: Options, g: Guide) {
  switch (g.status) {
    case 'followed':
      options.disabledGuides = [...options.disabledGuides, key(g)];
      break;
    case 'off':
      options.disabledGuides = options.disabledGuides.filter(other(g));
      break;
    case 'candidate':
      options.forcedGuides = [...options.forcedGuides, key(g)];
      break;
    case 'forced':
      options.forcedGuides = options.forcedGuides.filter(other(g));
      break;
  }
}
