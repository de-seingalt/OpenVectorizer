<!--
  Palette editor. Each large bubble is an output color; the small bubbles inside are the source
  colors (tints found in the image) it groups. Gestures:
  - drag a small bubble onto another large one: move that tint to that color;
  - drag a small bubble into empty space: make it a new color (split);
  - drag a large bubble onto another: merge the two colors;
  - click a large bubble: edit its color.
  Keyboard: Enter on a tint picks it, then Enter on a color moves it there (or on "New color").
  Every change becomes `options.paletteEdit`; the engine re-traces with it.
-->
<script lang="ts">
  import { Plus, RotateCcw, X } from '@lucide/svelte';
  import { Button } from '$lib/components/ui/button';
  import type { Options, Palette, PaletteEdit } from '$lib/engine';
  import { t } from '$lib/i18n';

  let { options = $bindable(), palette }: { options: Options; palette: Palette } = $props();

  type Drag = { kind: 'source' | 'group'; index: number; x: number; y: number; moved: boolean };
  let drag = $state<Drag | null>(null);
  let picked = $state<number | null>(null); // keyboard-picked source
  let selected = $state<number | null>(null); // group whose color is being edited

  const total = $derived(palette.sources.reduce((n, s) => n + s.pixels, 0) || 1);
  const members = $derived(
    palette.groups.map((_, g) => palette.sources.map((s, i) => ({ ...s, index: i })).filter((s) => s.group === g)),
  );

  const membersOf = (g: number) => members[g] ?? [];
  const groupOf = (source: number) => palette.sources[source]?.group ?? 0;
  const pixelsOf = (g: number) => membersOf(g).reduce((n, s) => n + s.pixels, 0);

  function groupSize(g: number) {
    const share = pixelsOf(g) / total;
    return Math.round(48 + 72 * Math.sqrt(share));
  }
  function sourceSize(g: number, pixels: number) {
    const inGroup = pixelsOf(g) || 1;
    const d = groupSize(g);
    return Math.round(Math.max(12, Math.min(d * 0.5, d * 0.6 * Math.sqrt(pixels / inGroup))));
  }
  /** Black or white, whichever reads better on `hex`. */
  function contrast(hex: string) {
    const c = (i: number) => parseInt(hex.slice(i, i + 2), 16) / 255;
    return 0.2126 * c(1) + 0.7152 * c(3) + 0.0722 * c(5) > 0.55 ? '#000000' : '#ffffff';
  }

  /** Apply a change to the current grouping and hand it to the engine. */
  function edit(change: (groups: number[], colors: (string | null)[]) => void) {
    const groups = palette.sources.map((s) => s.group);
    const colors = palette.groups.map((g) => (g.custom ? g.color : null));
    change(groups, colors);
    options.paletteEdit = { key: palette.key, groups, colors } satisfies PaletteEdit;
  }
  function moveSource(source: number, to: number) {
    if (groupOf(source) !== to) edit((groups) => (groups[source] = to));
  }
  function splitSource(source: number) {
    if (membersOf(groupOf(source)).length < 2) return; // already alone in its color
    edit((groups, colors) => {
      groups[source] = colors.length;
      colors.push(null);
    });
  }
  function mergeGroups(from: number, to: number) {
    if (from !== to) edit((groups) => groups.forEach((g, i) => g === from && (groups[i] = to)));
    selected = null;
  }
  function setColor(g: number, color: string | null) {
    edit((_, colors) => (colors[g] = color));
  }

  // --- pointer gestures -------------------------------------------------------------------
  function start(e: PointerEvent, kind: Drag['kind'], index: number) {
    if (e.button !== 0) return;
    e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag = { kind, index, x: e.clientX, y: e.clientY, moved: false };
  }
  function move(e: PointerEvent) {
    if (!drag) return;
    const moved = drag.moved || Math.hypot(e.clientX - drag.x, e.clientY - drag.y) > 4;
    drag = { ...drag, x: e.clientX, y: e.clientY, moved };
  }
  function end(e: PointerEvent) {
    const d = drag;
    drag = null;
    if (!d) return;
    if (!d.moved) {
      // A click: edit the color of the bubble (or of the tint's color).
      const g = d.kind === 'group' ? d.index : groupOf(d.index);
      selected = selected === g ? null : g;
      return;
    }
    const target = document.elementFromPoint(e.clientX, e.clientY);
    const groupEl = target?.closest<HTMLElement>('[data-group]');
    const inArea = target?.closest('[data-palette-area]');
    if (groupEl) {
      const to = Number(groupEl.dataset.group);
      if (d.kind === 'source') moveSource(d.index, to);
      else mergeGroups(d.index, to);
    } else if (inArea && d.kind === 'source') {
      splitSource(d.index);
    }
  }

  // --- keyboard ---------------------------------------------------------------------------
  function keySource(e: KeyboardEvent, i: number) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      e.stopPropagation();
      picked = picked === i ? null : i;
    }
  }
  function keyGroup(e: KeyboardEvent, g: number) {
    if (e.key !== 'Enter' && e.key !== ' ') return;
    e.preventDefault();
    if (picked !== null) {
      moveSource(picked, g);
      picked = null;
    } else {
      selected = selected === g ? null : g;
    }
  }
  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      picked = null;
      selected = null;
    }
  }

  const selectedGroup = $derived(selected === null ? undefined : palette.groups[selected]);
  const dragging = $derived(drag?.moved ? drag : null);
  const dragColor = $derived(
    dragging
      ? dragging.kind === 'source'
        ? palette.sources[dragging.index]?.color
        : palette.groups[dragging.index]?.color
      : null,
  );
</script>

<svelte:window onpointermove={move} onpointerup={end} onkeydown={onKeydown} />

<div class="flex flex-col gap-2">
  <div
    data-palette-area
    role="group"
    aria-label={t.paletteArea}
    class={[
      'relative flex min-h-36 flex-wrap content-center items-center justify-center gap-2 rounded-lg border p-3 transition-colors',
      dragging?.kind === 'source' ? 'border-dashed border-primary bg-primary/5' : 'bg-muted/40',
    ]}
  >
    {#each palette.groups as group, g (g)}
      {@const size = groupSize(g)}
      {@const ring = contrast(group.color)}
      <div
        data-group={g}
        role="button"
        tabindex="0"
        aria-label={`${t.colorLabel} ${group.color}, ${membersOf(g).length} ${membersOf(g).length > 1 ? t.tints : t.tintOne}`}
        aria-pressed={selected === g}
        class={[
          'flex shrink-0 cursor-grab touch-none flex-wrap content-center items-center justify-center gap-0.5 overflow-hidden rounded-full p-2 shadow-sm outline-offset-2 transition-transform select-none focus-visible:outline-2 focus-visible:outline-ring',
          selected === g && 'outline-2 outline-ring',
          dragging?.kind === 'group' && dragging.index === g && 'opacity-40',
        ]}
        style:width="{size}px"
        style:height="{size}px"
        style:background-color={group.color}
        onpointerdown={(e) => start(e, 'group', g)}
        onkeydown={(e) => keyGroup(e, g)}
      >
        {#each membersOf(g) as s (s.index)}
          {@const ss = sourceSize(g, s.pixels)}
          <button
            type="button"
            class={[
              'shrink-0 cursor-grab touch-none rounded-full border-2 outline-offset-1 focus-visible:outline-2 focus-visible:outline-ring',
              picked === s.index && 'outline-2 outline-ring',
              dragging?.kind === 'source' && dragging.index === s.index && 'opacity-30',
            ]}
            style:width="{ss}px"
            style:height="{ss}px"
            style:background-color={s.color}
            style:border-color={ring}
            aria-label={`${t.tint} ${s.color}`}
            aria-pressed={picked === s.index}
            onpointerdown={(e) => start(e, 'source', s.index)}
            onkeydown={(e) => keySource(e, s.index)}
          ></button>
        {/each}
      </div>
    {/each}
    {#if picked !== null}
      <Button variant="outline" size="sm" onclick={() => (splitSource(picked!), (picked = null))}>
        <Plus />
        {t.newColor}
      </Button>
    {/if}
  </div>

  {#if dragging && dragColor}
    <div
      class="pointer-events-none fixed z-50 size-8 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white shadow-lg"
      style:left="{dragging.x}px"
      style:top="{dragging.y}px"
      style:background-color={dragColor}
    ></div>
  {/if}

  {#if selectedGroup && selected !== null}
    {@const group = selectedGroup}
    <div class="flex items-center gap-2 rounded-lg border p-2">
      <label class="flex flex-1 items-center gap-2 text-sm">
        <input
          type="color"
          class="size-8 cursor-pointer rounded border bg-transparent"
          value={group.color}
          onchange={(e) => setColor(selected!, (e.currentTarget as HTMLInputElement).value)}
        />
        <span class="font-mono text-xs">{group.color}</span>
      </label>
      {#if group.custom}
        <Button variant="ghost" size="sm" onclick={() => setColor(selected!, null)}>
          <RotateCcw />
          {t.revertColor}
        </Button>
      {/if}
      <Button variant="ghost" size="icon-sm" aria-label={t.close} onclick={() => (selected = null)}>
        <X />
      </Button>
    </div>
  {/if}

  <p class="text-xs text-muted-foreground">{t.paletteHelp}</p>
</div>
