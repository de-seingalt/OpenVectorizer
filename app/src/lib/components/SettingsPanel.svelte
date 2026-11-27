<script lang="ts">
  import { ChevronRight, RotateCcw } from '@lucide/svelte';
  import { Button } from '$lib/components/ui/button';
  import * as Collapsible from '$lib/components/ui/collapsible';
  import { Label } from '$lib/components/ui/label';
  import { Slider } from '$lib/components/ui/slider';
  import { Switch } from '$lib/components/ui/switch';
  import * as ToggleGroup from '$lib/components/ui/toggle-group';
  import { DEFAULT_OPTIONS, type Options, type Preset } from '$lib/engine';
  import { t } from '$lib/i18n';

  let { options = $bindable(), palette = [] }: { options: Options; palette?: string[] } = $props();

  const MAX_SWATCHES = 48;
  const auto = $derived(options.maxColors === 0);
  function setAuto(on: boolean) {
    // Leaving automatic mode starts from the number of colors currently found.
    options.maxColors = on ? 0 : Math.min(32, Math.max(2, palette.length || 8));
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

  <section class="flex flex-col gap-3" aria-labelledby="colors-label">
    <div class="flex items-center justify-between">
      <span id="colors-label" class="text-sm leading-none font-medium">{t.colors}</span>
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
            max={32}
            step={1}
            bind:value={options.maxColors}
            label={t.colorsHint}
          />
          <span class="w-6 text-right text-xs text-muted-foreground tabular-nums">{options.maxColors}</span>
        </div>
      {/if}
      {#if palette.length > 0}
        <ul class="flex flex-wrap gap-1" aria-label={`${palette.length} ${t.colorsFound}`}>
          {#each palette.slice(0, MAX_SWATCHES) as color (color)}
            <li class="size-6 rounded-md border border-border shadow-xs" style:background-color={color} title={color}>
              <span class="sr-only">{color}</span>
            </li>
          {/each}
        </ul>
        <p class="text-xs text-muted-foreground">
          {palette.length}
          {t.colorsFound}{palette.length > MAX_SWATCHES ? ` (${MAX_SWATCHES} ${t.shown})` : ''}
        </p>
      {/if}
    {/if}
  </section>

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
