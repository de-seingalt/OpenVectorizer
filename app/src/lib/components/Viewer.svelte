<script lang="ts">
  // Zoomable / pannable comparison of the original raster and the SVG result.
  // The stage is laid out at its zoomed size (not CSS-scaled) so the SVG is
  // re-rendered crisply by the browser at every zoom level.
  import { Maximize, Minus, Plus } from '@lucide/svelte';
  import { Button } from '$lib/components/ui/button';
  import * as ToggleGroup from '$lib/components/ui/toggle-group';
  import type { CornerInfo, Guide, GuideKind } from '$lib/engine';
  import { t } from '$lib/i18n';

  type Mode = 'compare' | 'original' | 'vector';

  let {
    rasterUrl,
    svg,
    width,
    height,
    corners = [],
    showCorners = false,
    selectedCorner = $bindable(null),
    onToggleCorner,
    onSetCornerRadius,
    guides = [],
    showGuides = false,
    onToggleGuide,
  }: {
    rasterUrl: string;
    /** The SVG document of the result (drawn inline, as vectors, at screen resolution). */
    svg: string | null;
    width: number;
    height: number;
    /** Corner markers drawn over the preview when `showCorners` is on. */
    corners?: CornerInfo[];
    showCorners?: boolean;
    selectedCorner?: CornerInfo | null;
    /** Double-click on a marker. */
    onToggleCorner?: (corner: CornerInfo) => void;
    /** Radius set by dragging the selected corner's arc (0 = sharp). */
    onSetCornerRadius?: (corner: CornerInfo, radius: number) => void;
    /** Guides drawn over the preview when `showGuides` is on; a click turns one off or on. */
    guides?: Guide[];
    showGuides?: boolean;
    onToggleGuide?: (guide: Guide) => void;
  } = $props();

  const guideColor: Record<GuideKind, string> = {
    line: '#0891b2',
    arc: '#c026d3',
    circle: '#ea580c',
    ellipse: '#ea580c',
    symmetry: '#7c3aed',
    repeat: '#16a34a',
  };

  /** Path of a corner drawn as its two sides near the apex, joined by its arc when rounded. */
  function cornerShape(k: CornerInfo, r: number) {
    const [ix, iy] = k.din;
    const [ox, oy] = k.dout;
    const turn = Math.acos(Math.max(-1, Math.min(1, ix * ox + iy * oy)));
    const half = Math.max(1e-3, (Math.PI - turn) / 2);
    const round = r > 0.05;
    const t = round ? r / Math.tan(half) : 0;
    const t1 = { x: k.x - ix * t, y: k.y - iy * t };
    const t2 = { x: k.x + ox * t, y: k.y + oy * t };
    const leg = 14 / scale;
    const sweep = ix * oy - iy * ox > 0 ? 1 : 0;
    const f = (v: number) => Math.round(v * 100) / 100;
    const d =
      `M${f(t1.x - ix * leg)} ${f(t1.y - iy * leg)}L${f(t1.x)} ${f(t1.y)}` +
      (round ? `A${f(r)} ${f(r)} 0 0 ${sweep} ${f(t2.x)} ${f(t2.y)}` : '') +
      `L${f(t2.x + ox * leg)} ${f(t2.y + oy * leg)}`;
    // Middle of the arc, on the bisector, at r·(1/sin(half) − 1) from the apex.
    const bx = ox - ix;
    const by = oy - iy;
    const bl = Math.hypot(bx, by) || 1;
    const m = r * (1 / Math.sin(half) - 1);
    return {
      d,
      round,
      r,
      half,
      bis: { x: bx / bl, y: by / bl },
      handle: { x: k.x + (bx / bl) * m, y: k.y + (by / bl) * m },
    };
  }

  // Radius drag on the selected corner: previewed while dragging, applied on release.
  let dragging = $state<{ k: CornerInfo; r: number } | null>(null);
  function radiusAt(e: PointerEvent, k: CornerInfo) {
    const rect = box!.getBoundingClientRect();
    const p = { x: (e.clientX - rect.left - tx) / scale, y: (e.clientY - rect.top - ty) / scale };
    const g = cornerShape(k, 0);
    const m = Math.max(0, (p.x - k.x) * g.bis.x + (p.y - k.y) * g.bis.y);
    return Math.min(k.maxRadius, m / Math.max(1e-6, 1 / Math.sin(g.half) - 1));
  }
  function startRadiusDrag(e: PointerEvent, k: CornerInfo) {
    if (e.button !== 0) return;
    e.stopPropagation();
    dragging = { k, r: k.sharp ? 0 : k.radius };
  }
  function moveRadiusDrag(e: PointerEvent) {
    if (dragging) dragging = { ...dragging, r: radiusAt(e, dragging.k) };
  }
  function endRadiusDrag() {
    if (dragging) onSetCornerRadius?.(dragging.k, dragging.r);
    dragging = null;
  }

  // Content of the result document, without its own <svg> element.
  const inner = $derived(svg ? svg.slice(svg.indexOf('>') + 1, svg.lastIndexOf('</svg>')) : '');

  const isSelected = (k: CornerInfo) => selectedCorner !== null && k.x === selectedCorner.x && k.y === selectedCorner.y;
  const inGroup = (k: CornerInfo) => selectedCorner?.group != null && k.group === selectedCorner.group;

  let mode = $state<Mode>('compare');
  let split = $state(0.5);
  let scale = $state(1);
  let tx = $state(0);
  let ty = $state(0);
  let box: HTMLDivElement | undefined = $state();
  let boxW = $state(0);
  let boxH = $state(0);

  const MIN = 0.05;
  const MAX = 64;
  const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

  function fit() {
    if (!boxW || !boxH) return;
    // Only writes `scale`: the refit effect below must not depend on it, or every zoom
    // would trigger a refit that cancels it.
    const s = clamp(Math.min((boxW - 48) / width, (boxH - 96) / height), MIN, 8);
    scale = s;
    tx = (boxW - width * s) / 2;
    ty = (boxH - 40 - height * s) / 2;
  }

  function zoomAt(factor: number, mx = boxW / 2, my = boxH / 2) {
    const next = clamp(scale * factor, MIN, MAX);
    tx = mx - (mx - tx) * (next / scale);
    ty = my - (my - ty) * (next / scale);
    scale = next;
  }

  // Refit whenever a new image (size) arrives or the viewport size is first known.
  $effect(() => {
    void width;
    void height;
    if (boxW && boxH) fit();
  });

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const r = box!.getBoundingClientRect();
    zoomAt(Math.exp(-e.deltaY * 0.0015), e.clientX - r.left, e.clientY - r.top);
  }

  let drag: { kind: 'pan' | 'split'; x: number; y: number } | null = null;

  function onPointerDown(e: PointerEvent, kind: 'pan' | 'split') {
    if (e.button !== 0) return;
    e.stopPropagation();
    drag = { kind, x: e.clientX, y: e.clientY };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onPointerMove(e: PointerEvent) {
    if (!drag) return;
    if (drag.kind === 'pan') {
      tx += e.clientX - drag.x;
      ty += e.clientY - drag.y;
    } else {
      const r = box!.getBoundingClientRect();
      split = clamp((e.clientX - r.left - tx) / (width * scale), 0, 1);
    }
    drag = { ...drag, x: e.clientX, y: e.clientY };
  }

  const endDrag = () => (drag = null);
  const pixelated = $derived(scale >= 2);
</script>

<!-- Corner radius drag: followed anywhere on the page until the button is released. -->
<svelte:window onpointermove={moveRadiusDrag} onpointerup={endRadiusDrag} onpointercancel={() => (dragging = null)} />

<div
  bind:this={box}
  bind:clientWidth={boxW}
  bind:clientHeight={boxH}
  class="checker relative h-full w-full cursor-grab touch-none overflow-hidden select-none active:cursor-grabbing"
  onwheel={onWheel}
  onpointerdown={(e) => onPointerDown(e, 'pan')}
  onpointermove={onPointerMove}
  onpointerup={endDrag}
  onpointercancel={endDrag}
  role="application"
  aria-label="Aperçu"
>
  <div
    class="absolute shadow-lg"
    style:left="{tx}px"
    style:top="{ty}px"
    style:width="{width * scale}px"
    style:height="{height * scale}px"
  >
    {#if mode !== 'vector' || !svg}
      <img
        src={rasterUrl}
        alt={t.viewOriginal}
        draggable="false"
        class="absolute inset-0 h-full w-full"
        style:image-rendering={pixelated ? 'pixelated' : 'auto'}
      />
    {/if}
    {#if svg && mode !== 'original'}
      <!-- The result is drawn inline, covering the viewport only, with a viewBox on the visible
           part of the image: the browser draws the curves at screen resolution at any zoom (an
           <img> would be rasterized once, and blurred when enlarged). -->
      <svg
        class="pointer-events-none absolute"
        style:left="{-tx}px"
        style:top="{-ty}px"
        width={boxW}
        height={boxH}
        viewBox="{-tx / scale} {-ty / scale} {boxW / scale} {boxH / scale}"
        preserveAspectRatio="none"
        role="img"
        aria-label={t.viewVector}
        style:clip-path={mode === 'compare' ? `inset(0 0 0 ${Math.max(0, tx + width * scale * split)}px)` : 'none'}
      >
        <!-- eslint-disable-next-line svelte/no-at-html-tags -- engine output: paths with fill colours only -->
        {@html inner}
      </svg>
    {/if}
    {#if showGuides && svg}
      <svg
        class="pointer-events-none absolute inset-0 h-full w-full overflow-visible"
        viewBox="0 0 {width} {height}"
        role="group"
        aria-label={t.guideMarkers}
      >
        {#each guides as g, i (i)}
          <g
            class="pointer-events-auto cursor-pointer outline-none"
            role="button"
            tabindex={guides.length <= 300 ? 0 : -1}
            aria-label={`${t.guideKinds[g.kind]} ${t.guideStatus[g.status]} (${Math.round(g.x)}, ${Math.round(g.y)})`}
            aria-pressed={g.status === 'followed' || g.status === 'forced'}
            data-status={g.status}
            data-guide={i}
            onpointerdown={(e) => e.stopPropagation()}
            onclick={() => onToggleGuide?.(g)}
            onkeydown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                onToggleGuide?.(g);
              }
            }}
          >
            <!-- Wide invisible stroke: easier to click. -->
            <path d={g.d} fill="none" stroke="transparent" stroke-width="10" vector-effect="non-scaling-stroke" />
            <path
              d={g.d}
              fill="none"
              stroke={g.status === 'off' ? '#9ca3af' : guideColor[g.kind]}
              stroke-width={g.status === 'forced' ? 2.5 : g.status === 'followed' ? 1.5 : 1.25}
              stroke-opacity={g.status === 'candidate' ? 0.6 : 1}
              stroke-dasharray={g.status === 'followed' || g.status === 'forced'
                ? g.kind === 'symmetry'
                  ? `${6 / scale} ${3 / scale}`
                  : 'none'
                : `${2 / scale} ${3 / scale}`}
              vector-effect="non-scaling-stroke"
            />
          </g>
        {/each}
      </svg>
    {/if}
    {#if showCorners && svg}
      <svg
        class="pointer-events-none absolute inset-0 h-full w-full overflow-visible"
        viewBox="0 0 {width} {height}"
        role="group"
        aria-label={t.cornerMarkers}
      >
        {#each corners as k, i (i)}
          {@const sel = isSelected(k)}
          {@const g = cornerShape(k, dragging && isSelected(dragging.k) && sel ? dragging.r : k.sharp ? 0 : k.radius)}
          {@const color = k.edited ? '#2563eb' : g.round ? '#d97706' : '#16a34a'}
          <g
            class="pointer-events-auto cursor-pointer outline-none"
            role="button"
            tabindex={corners.length <= 300 ? 0 : -1}
            aria-label={`${k.sharp ? t.cornerSharp : t.cornerRound} (${Math.round(k.x)}, ${Math.round(k.y)})`}
            aria-pressed={sel}
            data-corner={i}
            onpointerdown={(e) => e.stopPropagation()}
            onclick={() => (selectedCorner = sel ? null : k)}
            ondblclick={() => onToggleCorner?.(k)}
            onkeydown={(e) => {
              if (e.key === 'Enter') selectedCorner = sel ? null : k;
              else if (e.key === ' ') {
                e.preventDefault();
                onToggleCorner?.(k);
              }
            }}
          >
            <!-- Wide invisible stroke: easier to click. -->
            <path d={g.d} fill="none" stroke="transparent" stroke-width="12" vector-effect="non-scaling-stroke" />
            <path
              d={g.d}
              fill="none"
              stroke={color}
              stroke-width={sel ? 3 : inGroup(k) ? 2.25 : 1.75}
              stroke-linejoin="round"
              vector-effect="non-scaling-stroke"
            />
          </g>
          {#if sel}
            <!-- Drag along the bisector to set the radius (to the apex: sharp). -->
            <circle
              class="pointer-events-auto cursor-move"
              role="slider"
              tabindex="-1"
              aria-label={t.cornerRadius}
              aria-valuemin={0}
              aria-valuemax={Math.round(k.maxRadius * 10) / 10}
              aria-valuenow={Math.round(g.r * 10) / 10}
              data-corner-handle
              cx={g.handle.x}
              cy={g.handle.y}
              r={5 / scale}
              fill="white"
              stroke={color}
              stroke-width={2 / scale}
              onpointerdown={(e) => startRadiusDrag(e, k)}
            />
          {/if}
        {/each}
      </svg>
    {/if}
  </div>

  {#if svg && mode === 'compare'}
    <!-- Split handle -->
    <div
      class="absolute top-0 bottom-0 z-10 w-6 -translate-x-1/2 cursor-ew-resize"
      style:left="{tx + width * scale * split}px"
      onpointerdown={(e) => onPointerDown(e, 'split')}
      role="slider"
      aria-label={t.viewCompare}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(split * 100)}
      tabindex="0"
      onkeydown={(e) => {
        if (e.key === 'ArrowLeft') split = clamp(split - 0.05, 0, 1);
        if (e.key === 'ArrowRight') split = clamp(split + 0.05, 0, 1);
      }}
    >
      <div class="mx-auto h-full w-0.5 bg-primary shadow"></div>
      <div
        class="absolute top-1/2 left-1/2 flex size-7 -translate-1/2 items-center justify-center rounded-full bg-primary text-xs text-primary-foreground shadow-md"
      >
        ⇆
      </div>
    </div>
    <span class="absolute top-3 left-3 rounded bg-background/80 px-2 py-0.5 text-xs">{t.viewOriginal}</span>
    <span class="absolute top-3 right-3 rounded bg-background/80 px-2 py-0.5 text-xs">{t.viewVector}</span>
  {/if}

  <!-- Toolbar -->
  <div
    class="absolute bottom-3 left-1/2 flex -translate-x-1/2 items-center gap-2 rounded-lg border bg-background/90 p-1 shadow-sm backdrop-blur"
    onpointerdown={(e) => e.stopPropagation()}
    role="toolbar"
    tabindex="-1"
  >
    <ToggleGroup.Root
      type="single"
      size="sm"
      value={mode}
      onValueChange={(v) => {
        if (v) mode = v as Mode;
      }}
    >
      <ToggleGroup.Item value="compare">{t.viewCompare}</ToggleGroup.Item>
      <ToggleGroup.Item value="original">{t.viewOriginal}</ToggleGroup.Item>
      <ToggleGroup.Item value="vector" disabled={!svg}>{t.viewVector}</ToggleGroup.Item>
    </ToggleGroup.Root>
    <div class="h-5 w-px bg-border"></div>
    <Button variant="ghost" size="icon-sm" aria-label={t.zoomOut} onclick={() => zoomAt(1 / 1.25)}><Minus /></Button>
    <span class="w-12 text-center text-xs tabular-nums">{Math.round(scale * 100)} %</span>
    <Button variant="ghost" size="icon-sm" aria-label={t.zoomIn} onclick={() => zoomAt(1.25)}><Plus /></Button>
    <Button variant="ghost" size="icon-sm" aria-label={t.fit} title={t.fit} onclick={fit}><Maximize /></Button>
  </div>
</div>

<style>
  .checker {
    background-color: var(--muted);
    background-image:
      linear-gradient(45deg, var(--border) 25%, transparent 25%),
      linear-gradient(-45deg, var(--border) 25%, transparent 25%),
      linear-gradient(45deg, transparent 75%, var(--border) 75%),
      linear-gradient(-45deg, transparent 75%, var(--border) 75%);
    background-size: 20px 20px;
    background-position:
      0 0,
      0 10px,
      10px -10px,
      -10px 0;
  }
</style>
