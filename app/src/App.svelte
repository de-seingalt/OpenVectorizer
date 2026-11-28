<script lang="ts">
  import {
    ClipboardPaste,
    Download,
    FolderOpen,
    ImagePlus,
    Loader2,
    Moon,
    Sun,
    Copy,
    ChevronDown,
  } from '@lucide/svelte';
  import { ModeWatcher, toggleMode } from 'mode-watcher';
  import { toast } from 'svelte-sonner';
  import SettingsPanel from '$lib/components/SettingsPanel.svelte';
  import Viewer from '$lib/components/Viewer.svelte';
  import { Button } from '$lib/components/ui/button';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
  import { Toaster } from '$lib/components/ui/sonner';
  import { editCorners, findCorner, targets } from '$lib/corners';
  import { toggleGuide } from '$lib/guides';
  import {
    DEFAULT_OPTIONS,
    Engine,
    SupersededError,
    type CornerInfo,
    type Guide,
    type Options,
    type Palette,
    type Stats,
  } from '$lib/engine';
  import { t } from '$lib/i18n';
  import { ACCEPTED_TYPES, baseName, loadImage, type LoadedImage } from '$lib/image';
  import { pickImage, saveText } from '$lib/platform';

  const engine = new Engine();

  let image = $state<LoadedImage | null>(null);
  let options = $state<Options>({ ...DEFAULT_OPTIONS });
  let svg = $state<string | null>(null);
  let svgUrl = $state<string | null>(null);
  let stats = $state<Stats | null>(null);
  let palette = $state<Palette | null>(null);
  let corners = $state<CornerInfo[]>([]);
  let showCorners = $state(false);
  let selectedCorner = $state<CornerInfo | null>(null);
  let cornerGroup = $state(true); // edits apply to the whole group of the corner
  let guides = $state<Guide[]>([]);
  let showGuides = $state(false);
  let ms = $state(0);
  let working = $state(false);
  let dragging = $state(false);

  async function openBlob(blob: Blob, name: string) {
    try {
      const loaded = await loadImage(blob, name);
      if (image) URL.revokeObjectURL(image.url);
      image = loaded;
      if (loaded.pixels.width !== loaded.originalWidth) {
        toast.info(`${t.downscaled} ${loaded.pixels.width}×${loaded.pixels.height}`);
      }
    } catch {
      toast.error(t.invalidImage);
    }
  }

  async function openFile() {
    const file = await pickImage();
    if (file) await openBlob(file, file.name);
  }

  async function pasteFromClipboard() {
    try {
      for (const item of await navigator.clipboard.read()) {
        const type = item.types.find((ty) => ty.startsWith('image/'));
        if (type) return openBlob(await item.getType(type), 'clipboard.png');
      }
      toast.error(t.invalidImage);
    } catch (e) {
      toast.error(String(e));
    }
  }

  async function openExample() {
    const res = await fetch('./samples/example.svg');
    await openBlob(await res.blob(), 'example.svg');
  }

  // Re-vectorize (debounced) whenever the image or an option changes.
  $effect(() => {
    const img = image;
    const opts = $state.snapshot(options);
    if (!img) return;
    const timer = setTimeout(async () => {
      working = true;
      try {
        const res = await engine.vectorize(img.pixels, opts);
        if (svgUrl) URL.revokeObjectURL(svgUrl);
        svg = res.output.svg;
        svgUrl = URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml' }));
        stats = res.output.stats;
        palette = res.output.palette;
        corners = res.output.corners;
        guides = res.output.guides;
        // Follow the selected corner into the new result.
        if (selectedCorner) selectedCorner = findCorner(corners, selectedCorner);
        ms = res.ms;
        working = false;
      } catch (e) {
        if (e instanceof SupersededError) return;
        working = false;
        toast.error(`${t.error} : ${e instanceof Error ? e.message : String(e)}`);
      }
    }, 120);
    return () => clearTimeout(timer);
  });

  function toggleCorner(k: CornerInfo) {
    editCorners(options, targets(corners, k, cornerGroup), { sharp: !k.sharp, radius: null });
  }

  function setCornerRadius(k: CornerInfo, radius: number) {
    const change = radius < 0.05 ? { sharp: true, radius: null } : { sharp: false, radius };
    editCorners(options, targets(corners, k, cornerGroup), change);
  }

  async function exportSvg() {
    if (!svg || !image) return;
    if (await saveText(svg, `${baseName(image.name)}.svg`, 'image/svg+xml')) toast.success(t.saved);
  }

  async function copySvg() {
    if (!svg) return;
    await navigator.clipboard.writeText(svg);
    toast.success(t.copied);
  }

  function onPaste(e: ClipboardEvent) {
    const file = [...(e.clipboardData?.files ?? [])].find((f) => f.type.startsWith('image/'));
    if (file) {
      e.preventDefault();
      void openBlob(file, file.name || 'clipboard.png');
    }
  }

  function onDrop(e: DragEvent) {
    e.preventDefault();
    dragging = false;
    const file = [...(e.dataTransfer?.files ?? [])].find((f) => ACCEPTED_TYPES.includes(f.type));
    if (file) void openBlob(file, file.name);
    else toast.error(t.invalidImage);
  }

  function onKeydown(e: KeyboardEvent) {
    const mod = e.ctrlKey || e.metaKey;
    if (e.key === 'Escape') selectedCorner = null;
    if (mod && e.key === 'o') {
      e.preventDefault();
      void openFile();
    } else if (mod && e.key === 's') {
      e.preventDefault();
      void exportSvg();
    }
  }

  const kb = (n: number) => (n < 1024 ? `${n} o` : `${(n / 1024).toFixed(1)} Ko`);
</script>

<svelte:window
  onpaste={onPaste}
  onkeydown={onKeydown}
  ondragover={(e) => {
    e.preventDefault();
    dragging = true;
  }}
  ondragleave={(e) => {
    if (!e.relatedTarget) dragging = false;
  }}
  ondrop={onDrop}
/>

<ModeWatcher />
<Toaster position="bottom-right" richColors />

<div class="flex h-screen flex-col bg-background text-foreground">
  <!-- Header -->
  <header class="flex h-14 shrink-0 items-center gap-2 border-b px-4">
    <img src="./favicon.svg" alt="" class="size-7" />
    <h1 class="mr-4 text-base font-semibold tracking-tight">OpenVectorizer</h1>
    <Button variant="outline" size="sm" onclick={openFile} title="Ctrl+O"><FolderOpen /> {t.open}</Button>
    <Button variant="ghost" size="sm" onclick={pasteFromClipboard} title="Ctrl+V"><ClipboardPaste /> {t.paste}</Button>
    <div class="flex-1"></div>
    <Button variant="ghost" size="icon" aria-label={t.theme} onclick={toggleMode}>
      <Sun class="dark:hidden" /><Moon class="hidden dark:block" />
    </Button>
    <div class="flex">
      <Button size="sm" class="rounded-r-none" disabled={!svg} onclick={exportSvg} title="Ctrl+S">
        <Download />
        {t.exportSvg}
      </Button>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger disabled={!svg}>
          {#snippet child({ props })}
            <Button {...props} size="sm" class="rounded-l-none border-l border-l-white/20 px-2" aria-label={t.export}>
              <ChevronDown />
            </Button>
          {/snippet}
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="end">
          <DropdownMenu.Item onclick={exportSvg}><Download /> {t.exportSvg}</DropdownMenu.Item>
          <DropdownMenu.Item onclick={copySvg}><Copy /> {t.copySvg}</DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    </div>
  </header>

  <div class="flex min-h-0 flex-1">
    <!-- Main area -->
    <main class="relative min-w-0 flex-1">
      {#if image}
        <Viewer
          rasterUrl={image.url}
          {svgUrl}
          width={image.pixels.width}
          height={image.pixels.height}
          {corners}
          {showCorners}
          bind:selectedCorner
          onToggleCorner={toggleCorner}
          onSetCornerRadius={setCornerRadius}
          {guides}
          {showGuides}
          onToggleGuide={(g) => toggleGuide(options, g)}
        />
      {:else}
        <div class="flex h-full items-center justify-center p-8">
          <button
            type="button"
            onclick={openFile}
            class="flex w-full max-w-xl flex-col items-center gap-4 rounded-2xl border-2 border-dashed p-12 text-center transition-colors hover:border-primary hover:bg-muted/50"
          >
            <ImagePlus class="size-12 text-muted-foreground" />
            <span class="text-xl font-semibold">{t.dropTitle}</span>
            <span class="text-sm text-muted-foreground">{t.dropHint}</span>
          </button>
        </div>
        <div class="absolute bottom-6 flex w-full justify-center">
          <Button variant="link" onclick={openExample}>{t.tryExample}</Button>
        </div>
      {/if}

      {#if dragging}
        <div
          class="pointer-events-none absolute inset-3 z-20 flex items-center justify-center rounded-2xl border-2 border-dashed border-primary bg-primary/10"
        >
          <span class="text-lg font-semibold">{t.dropTitle}</span>
        </div>
      {/if}
    </main>

    <!-- Settings -->
    <aside class="w-80 shrink-0 overflow-y-auto border-l">
      <SettingsPanel
        bind:options
        {palette}
        {stats}
        {corners}
        bind:showCorners
        bind:selectedCorner
        bind:cornerGroup
        {guides}
        bind:showGuides
      />
    </aside>
  </div>

  <!-- Status bar -->
  <footer class="flex h-8 shrink-0 items-center gap-4 border-t px-4 text-xs text-muted-foreground tabular-nums">
    {#if image}
      <span>{image.name}</span>
      <span>{image.pixels.width}×{image.pixels.height} px</span>
      {#if working}
        <span class="flex items-center gap-1"><Loader2 class="size-3 animate-spin" /> {t.working}</span>
      {:else if stats}
        <span>→ {stats.pathCount} {t.shapes} · {kb(stats.bytes)} · {Math.round(ms)} ms</span>
      {/if}
    {:else}
      <span>{t.privacy}</span>
    {/if}
  </footer>
</div>
