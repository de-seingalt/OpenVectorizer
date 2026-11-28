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
    svgUrl,
    width,
    height,
    corners = [],
    showCorners = false,
    selectedCorner = $bindable(null),
    onToggleCorner,
    guides = [],
    showGuides = false,
    onToggleGuide,
  }: {
    rasterUrl: string;
    svgUrl: string | null;
    width: number;
    height: number;
    /** Corner markers drawn over the preview when `showCorners` is on. */
    corners?: CornerInfo[];
    showCorners?: boolean;
    selectedCorner?: CornerInfo | null;
    /** Double-click on a marker. */
    onToggleCorner?: (corner: CornerInfo) => void;
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
  };

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
    const s = Math.min((boxW - 48) / width, (boxH - 96) / height);
    scale = clamp(s, MIN, 8);
    tx = (boxW - width * scale) / 2;
    ty = (boxH - 40 - height * scale) / 2;
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
  // Markers keep a constant size on screen.
  const mark = $derived(6 / scale);
</script>

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
    {#if mode !== 'vector' || !svgUrl}
      <img
        src={rasterUrl}
        alt={t.viewOriginal}
        draggable="false"
        class="absolute inset-0 h-full w-full"
        style:image-rendering={pixelated ? 'pixelated' : 'auto'}
      />
    {/if}
    {#if svgUrl && mode !== 'original'}
      <img
        src={svgUrl}
        alt={t.viewVector}
        draggable="false"
        class="absolute inset-0 h-full w-full"
        style:clip-path={mode === 'compare' ? `inset(0 0 0 ${split * 100}%)` : 'none'}
      />
    {/if}
    {#if showGuides && svgUrl}
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
            aria-label={`${t.guideKinds[g.kind]} (${Math.round(g.x)}, ${Math.round(g.y)})`}
            aria-pressed={g.enabled}
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
              stroke={g.enabled ? guideColor[g.kind] : '#9ca3af'}
              stroke-width={g.enabled ? 1.5 : 1.25}
              stroke-dasharray={g.enabled && g.kind !== 'symmetry' ? 'none' : `${4 / scale} ${3 / scale}`}
              vector-effect="non-scaling-stroke"
            />
          </g>
        {/each}
      </svg>
    {/if}
    {#if showCorners && svgUrl}
      <svg
        class="pointer-events-none absolute inset-0 h-full w-full overflow-visible"
        viewBox="0 0 {width} {height}"
        role="group"
        aria-label={t.cornerMarkers}
      >
        {#each corners as k, i (i)}
          {@const sel = isSelected(k)}
          {@const r = sel ? mark * 1.4 : mark}
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
            <!-- Larger invisible hit area. -->
            <circle cx={k.x} cy={k.y} r={mark * 1.8} fill="transparent" />
            {#if k.sharp}
              <rect
                x={k.x - r * 0.75}
                y={k.y - r * 0.75}
                width={r * 1.5}
                height={r * 1.5}
                transform="rotate(45 {k.x} {k.y})"
                class={sel ? 'fill-primary' : k.edited ? 'fill-amber-400' : 'fill-white'}
                stroke={inGroup(k) ? 'var(--primary)' : '#111'}
                stroke-width={(inGroup(k) ? 2.5 : 1.25) / scale}
              />
            {:else}
              <circle
                cx={k.x}
                cy={k.y}
                r={r * 0.7}
                class={sel ? 'fill-primary' : k.edited ? 'fill-amber-400/80' : 'fill-white/40'}
                stroke={inGroup(k) ? 'var(--primary)' : '#111'}
                stroke-width={(inGroup(k) ? 2.5 : 1.25) / scale}
              />
            {/if}
          </g>
        {/each}
      </svg>
    {/if}
  </div>

  {#if svgUrl && mode === 'compare'}
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
      <ToggleGroup.Item value="vector" disabled={!svgUrl}>{t.viewVector}</ToggleGroup.Item>
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
