<script lang="ts">
  import { ChevronRight, RotateCcw, X } from '@lucide/svelte';
  import { Button } from '$lib/components/ui/button';
  import * as Collapsible from '$lib/components/ui/collapsible';
  import { Label } from '$lib/components/ui/label';
  import { Slider } from '$lib/components/ui/slider';
  import { Switch } from '$lib/components/ui/switch';
  import * as ToggleGroup from '$lib/components/ui/toggle-group';
  import { editCorners, targets } from '$lib/corners';
  import {
    DEFAULT_OPTIONS,
    type CornerInfo,
    type CornerMode,
    type Guide,
    type Options,
    type Palette,
    type Preset,
    type Stats,
  } from '$lib/engine';
  import PaletteEditor from './PaletteEditor.svelte';
  import { t } from '$lib/i18n';

  let {
    options = $bindable(),
    palette = null,
    stats = null,
    corners = [],
    showCorners = $bindable(false),
    selectedCorner = $bindable(null),
    cornerGroup = $bindable(true),
    guides = [],
    showGuides = $bindable(false),
  }: {
    options: Options;
    palette?: Palette | null;
    stats?: Stats | null;
    corners?: CornerInfo[];
    showCorners?: boolean;
    selectedCorner?: CornerInfo | null;
    cornerGroup?: boolean;
    guides?: Guide[];
    showGuides?: boolean;
  } = $props();
  const guidesOff = $derived(guides.filter((g) => !g.enabled).length);

  // --- selected corner ------------------------------------------------------------------
  const groupSize = $derived(
    selectedCorner?.group != null ? corners.filter((k) => k.group === selectedCorner!.group).length : 1,
  );
  const selection = $derived(selectedCorner ? targets(corners, selectedCorner, cornerGroup) : []);
  const anyEdited = $derived(selection.some((k) => k.edited));
  function setSharp(sharp: boolean) {
    editCorners(options, selection, { sharp, radius: null });
  }
  function setRadius(radius: number) {
    editCorners(options, selection, { sharp: false, radius });
  }

  const auto = $derived(options.maxColors === 0);
  const maxSources = $derived(Math.max(2, palette?.sources.length ?? 24));
  function setAuto(on: boolean) {
    // Leaving automatic mode starts from the number of colors currently found.
    options.paletteEdit = null;
    options.maxColors = on ? 0 : Math.min(maxSources, Math.max(2, palette?.groups.length ?? 8));
  }
  function setMaxColors(v: number) {
    // A new color count regroups the tints automatically: manual edits are dropped.
    options.paletteEdit = null;
    options.maxColors = v;
  }

  const presets: { value: Preset; label: string }[] = [
    { value: 'logo', label: t.presetLogo },
    { value: 'illustration', label: t.presetIllustration },
    { value: 'photo', label: t.presetPhoto },
  ];
  const pct = (v: number) => `${Math.round(v * 100)} %`;
</script>

<div class="flex flex-col gap-6 p-4">
  <section class="flex flex-col gap-2">
    <Label class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">{t.preset}</Label>
    <ToggleGroup.Root
      type="single"
      variant="outline"
      class="w-full"
      value={options.preset}
      onValueChange={(v) => {
        if (v) options.preset = v as Preset;
      }}
    >
      {#each presets as p (p.value)}
        <ToggleGroup.Item value={p.value} class="flex-1">{p.label}</ToggleGroup.Item>
      {/each}
    </ToggleGroup.Root>
  </section>

  <section class="flex flex-col gap-3">
    <div class="flex items-baseline justify-between">
      <Label for="detail">{t.detail}</Label>
      <span class="text-xs text-muted-foreground tabular-nums">{pct(options.detail)}</span>
    </div>
    <Slider id="detail" label={t.detail} type="single" min={0} max={1} step={0.05} bind:value={options.detail} />
    <p class="text-xs text-muted-foreground">{t.detailHint}</p>
  </section>

  <section class="flex flex-col gap-3" aria-labelledby="colors-label">
    <div class="flex items-center justify-between">
      <span id="colors-label" class="text-sm leading-none font-medium">
        {t.colors}
        {#if palette && options.preset !== 'photo'}
          <span class="ml-1 text-xs font-normal text-muted-foreground tabular-nums">{palette.groups.length}</span>
        {/if}
      </span>
      {#if options.preset !== 'photo'}
        <div class="flex items-center gap-2">
          <Label for="colors-auto" class="text-xs font-normal text-muted-foreground">{t.colorsAuto}</Label>
          <Switch id="colors-auto" checked={auto} onCheckedChange={setAuto} />
        </div>
      {/if}
    </div>
    {#if options.preset === 'photo'}
      <p class="text-xs text-muted-foreground">{t.colorsPhoto}</p>
    {:else}
      {#if !auto}
        <div class="flex items-center gap-3">
          <Slider
            id="colors"
            type="single"
            min={2}
            max={maxSources}
            step={1}
            value={options.maxColors}
            onValueChange={setMaxColors}
            label={t.colorsHint}
          />
          <span class="w-6 text-right text-xs text-muted-foreground tabular-nums">{options.maxColors}</span>
        </div>
      {/if}
      {#if palette && palette.groups.length > 0}
        <PaletteEditor bind:options {palette} />
        {#if options.paletteEdit}
          <Button variant="outline" size="sm" onclick={() => (options.paletteEdit = null)}>
            <RotateCcw />
            {t.resetPalette}
          </Button>
        {/if}
      {/if}
    {/if}
  </section>

  <section class="flex flex-col gap-3" aria-labelledby="corners-label">
    <div class="flex items-center justify-between">
      <span id="corners-label" class="text-sm leading-none font-medium">
        {t.corners}
        {#if stats && options.preset !== 'photo'}
          <span class="ml-1 text-xs font-normal text-muted-foreground tabular-nums">
            {stats.sharpCorners}
            {t.sharpOf}
            {stats.corners}
          </span>
        {/if}
      </span>
    </div>
    {#if options.preset === 'photo'}
      <p class="text-xs text-muted-foreground">{t.cornersPhoto}</p>
    {:else}
      <ToggleGroup.Root
        type="single"
        variant="outline"
        size="sm"
        class="w-full"
        aria-label={t.corners}
        value={options.corners.sharpCount === null ? options.corners.mode : ''}
        onValueChange={(v) => {
          if (v) options.corners = { ...options.corners, mode: v as CornerMode, sharpCount: null };
        }}
      >
        <ToggleGroup.Item value="automatic" class="flex-1">{t.cornersAuto}</ToggleGroup.Item>
        <ToggleGroup.Item value="sharp" class="flex-1">{t.cornersSharp}</ToggleGroup.Item>
        <ToggleGroup.Item value="round" class="flex-1">{t.cornersRound}</ToggleGroup.Item>
      </ToggleGroup.Root>

      {#if stats && stats.corners > 0}
        <div class="flex flex-col gap-2">
          <div class="flex items-baseline justify-between">
            <Label for="sharp-count">{t.sharpCount}</Label>
            <span class="text-xs text-muted-foreground tabular-nums">
              {options.corners.sharpCount ?? stats.sharpCorners} / {stats.corners}
            </span>
          </div>
          <Slider
            id="sharp-count"
            label={t.sharpCount}
            type="single"
            min={0}
            max={stats.corners}
            step={1}
            value={options.corners.sharpCount ?? stats.sharpCorners}
            onValueChange={(v) => (options.corners = { ...options.corners, sharpCount: v })}
          />
          <div class="flex justify-between text-xs text-muted-foreground">
            <span>{t.fewerSharp}</span><span>{t.moreSharp}</span>
          </div>
        </div>
      {/if}

      <div class="flex flex-col gap-2">
        <div class="flex items-baseline justify-between">
          <Label for="radius">{t.radius}</Label>
          <span class="text-xs text-muted-foreground tabular-nums">{Math.round(options.corners.radius * 100)}</span>
        </div>
        <Slider
          id="radius"
          label={t.radius}
          type="single"
          min={-1}
          max={1}
          step={0.05}
          value={options.corners.radius}
          onValueChange={(v) => (options.corners = { ...options.corners, radius: v })}
        />
        <div class="flex justify-between text-xs text-muted-foreground">
          <span>{t.sharper}</span><span>{t.bestFit}</span><span>{t.rounder}</span>
        </div>
      </div>

      {#if stats && stats.corners > 0}
        <div class="flex items-start justify-between gap-3">
          <div class="flex flex-col gap-1">
            <Label for="show-corners">{t.showCorners}</Label>
            <p class="text-xs text-muted-foreground">{t.showCornersHint}</p>
          </div>
          <Switch
            id="show-corners"
            checked={showCorners}
            onCheckedChange={(c) => {
              showCorners = c;
              if (!c) selectedCorner = null;
            }}
          />
        </div>
      {/if}

      {#if selectedCorner}
        {@const k = selectedCorner}
        <div class="flex flex-col gap-3 rounded-lg border p-3" role="group" aria-labelledby="corner-title">
          <div class="flex items-center justify-between">
            <span id="corner-title" class="text-sm font-medium">{t.selectedCorner}</span>
            <Button variant="ghost" size="icon-sm" aria-label={t.close} onclick={() => (selectedCorner = null)}>
              <X />
            </Button>
          </div>
          {#if groupSize > 1}
            <div class="flex items-center justify-between gap-2">
              <Label for="corner-group" class="text-xs font-normal">
                {t.wholeGroup.replace('{n}', String(groupSize))}
              </Label>
              <Switch id="corner-group" bind:checked={cornerGroup} />
            </div>
          {/if}
          <ToggleGroup.Root
            type="single"
            variant="outline"
            size="sm"
            class="w-full"
            aria-label={t.selectedCorner}
            value={k.sharp ? 'sharp' : 'round'}
            onValueChange={(v) => {
              if (v) setSharp(v === 'sharp');
            }}
          >
            <ToggleGroup.Item value="sharp" class="flex-1">{t.cornerSharp}</ToggleGroup.Item>
            <ToggleGroup.Item value="round" class="flex-1">{t.cornerRound}</ToggleGroup.Item>
          </ToggleGroup.Root>
          {#if !k.sharp && k.maxRadius > 0.05}
            <div class="flex flex-col gap-2">
              <div class="flex items-baseline justify-between">
                <Label for="corner-radius">{t.cornerRadius}</Label>
                <span class="text-xs text-muted-foreground tabular-nums">{k.radius.toFixed(1)} px</span>
              </div>
              <Slider
                id="corner-radius"
                label={t.cornerRadius}
                type="single"
                min={0}
                max={Math.round(k.maxRadius * 10) / 10}
                step={0.1}
                value={Math.min(k.radius, k.maxRadius)}
                onValueChange={setRadius}
              />
            </div>
          {/if}
          {#if anyEdited}
            <Button variant="ghost" size="sm" onclick={() => editCorners(options, selection, null)}>
              <RotateCcw />
              {t.revertCorner}
            </Button>
          {/if}
        </div>
      {/if}

      {#if options.corners.edits.length > 0}
        <Button variant="outline" size="sm" onclick={() => (options.corners = { ...options.corners, edits: [] })}>
          <RotateCcw />
          {t.resetCorners} ({options.corners.edits.length})
        </Button>
      {/if}
    {/if}
  </section>

  {#if options.preset !== 'photo'}
    <section class="flex flex-col gap-3">
      <div class="flex items-baseline justify-between">
        <Label for="geometry">{t.geometry}</Label>
        <span class="text-xs text-muted-foreground tabular-nums">{pct(options.geometry)}</span>
      </div>
      <Slider
        id="geometry"
        label={t.geometry}
        type="single"
        min={0}
        max={1}
        step={0.05}
        bind:value={options.geometry}
      />
      <div class="flex justify-between text-xs text-muted-foreground">
        <span>{t.geometryOff}</span><span>{t.geometryStrong}</span>
      </div>
      <p class="text-xs text-muted-foreground">{t.geometryHint}</p>
      <div class="flex items-start justify-between gap-3">
        <div class="flex flex-col gap-1">
          <Label for="show-guides">
            {t.showGuides}
            <span id="guides-count" class="ml-1 text-xs font-normal text-muted-foreground tabular-nums">
              {guides.length}{#if guidesOff > 0}&nbsp;· {guidesOff} {t.guidesOff}{/if}
            </span>
          </Label>
          <p class="text-xs text-muted-foreground">{t.showGuidesHint}</p>
        </div>
        <Switch id="show-guides" bind:checked={showGuides} />
      </div>
      {#if options.disabledGuides.length > 0}
        <Button variant="outline" size="sm" onclick={() => (options.disabledGuides = [])}>
          <RotateCcw />
          {t.resetGuides} ({options.disabledGuides.length})
        </Button>
      {/if}
    </section>
  {/if}

  <Collapsible.Root>
    <Collapsible.Trigger
      class="group flex w-full items-center gap-1 text-xs font-semibold tracking-wide text-muted-foreground uppercase hover:text-foreground"
    >
      <ChevronRight class="size-4 transition-transform group-data-[state=open]:rotate-90" />
      {t.advanced}
    </Collapsible.Trigger>
    <Collapsible.Content class="flex flex-col gap-5 pt-4">
      <section class="flex flex-col gap-3">
        <div class="flex items-baseline justify-between">
          <Label for="smoothness">{t.smoothness}</Label>
          <span class="text-xs text-muted-foreground tabular-nums">{pct(options.smoothness)}</span>
        </div>
        <Slider
          id="smoothness"
          label={t.smoothness}
          type="single"
          min={0}
          max={1}
          step={0.05}
          bind:value={options.smoothness}
        />
        <p class="text-xs text-muted-foreground">{t.smoothnessHint}</p>
      </section>
      <section class="flex flex-col gap-3">
        <div class="flex items-baseline justify-between">
          <Label for="minarea">{t.minArea}</Label>
          <span class="text-xs text-muted-foreground tabular-nums">{options.minAreaPx}</span>
        </div>
        <Slider id="minarea" label={t.minArea} type="single" min={0} max={64} step={1} bind:value={options.minAreaPx} />
      </section>
      <section class="flex items-start justify-between gap-3">
        <div class="flex flex-col gap-1">
          <Label for="cutout">{t.cutout}</Label>
          <p class="text-xs text-muted-foreground">{t.cutoutHint}</p>
        </div>
        <Switch
          id="cutout"
          checked={options.stacking === 'cutout'}
          onCheckedChange={(c) => (options.stacking = c ? 'cutout' : 'stacked')}
        />
      </section>
      <Button variant="outline" size="sm" onclick={() => (options = { ...DEFAULT_OPTIONS })}>
        <RotateCcw />
        {t.reset}
      </Button>
    </Collapsible.Content>
  </Collapsible.Root>
</div>
