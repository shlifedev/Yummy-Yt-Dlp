<script lang="ts">
  import type { AdvancedOptions } from "$lib/bindings"
  import {
    CONTAINER_FORMATS,
    MAX_SLEEP_INTERVAL,
    SPONSORBLOCK_CATEGORIES,
    SUB_CONVERT_FORMATS,
    validateAdvancedField,
    type AdvancedTextField,
  } from "$lib/advanced"
  import { t } from "$lib/i18n/index.svelte"

  let {
    advanced = $bindable(),
    activeAdvancedCount,
    onClose,
    onSave,
    onReset,
    showTooltip,
    hideTooltip,
  }: {
    advanced: AdvancedOptions
    activeAdvancedCount: number
    onClose: () => void
    onSave: () => void
    onReset: () => void
    showTooltip: (event: MouseEvent, text: string) => void
    hideTooltip: () => void
  } = $props()

  const selCls = "bg-yt-bg border border-yt-border rounded px-1.5 py-0.5 text-xs text-yt-text focus:ring-1 focus:ring-yt-primary focus:outline-none cursor-pointer"
  const txtCls = "bg-yt-bg border border-yt-border rounded px-2 py-0.5 text-xs text-yt-text w-28 focus:ring-1 focus:ring-yt-primary focus:outline-none"
  const numCls = "bg-yt-bg border border-yt-border rounded px-2 py-0.5 text-xs text-yt-text w-16 focus:ring-1 focus:outline-none focus:ring-yt-primary"

  function saveAdvancedText(field: AdvancedTextField) {
    if (validateAdvancedField(field, advanced[field])) onSave()
  }

  function toggleSponsorblockCategory(cat: string) {
    advanced.sponsorblockCategories = advanced.sponsorblockCategories.includes(cat)
      ? advanced.sponsorblockCategories.filter((c) => c !== cat)
      : [...advanced.sponsorblockCategories, cat]
    onSave()
  }

  function setConcurrentFragments(v: string) {
    advanced.concurrentFragments = Math.max(1, Math.min(16, parseInt(v) || 1))
    onSave()
  }

  function setRetries(v: string) {
    advanced.retries = v.trim() === "" ? null : Math.max(0, Math.min(100, parseInt(v) || 0))
    onSave()
  }

  function setSleepInterval(v: string) {
    advanced.sleepInterval = Math.max(0, Math.min(MAX_SLEEP_INTERVAL, parseInt(v) || 0))
    onSave()
  }
</script>

{#snippet advLabel(text: string, help: string)}
  <span role="note" class="cursor-help shrink-0 text-yt-text-secondary" onmouseenter={(e) => showTooltip(e, help)} onmouseleave={hideTooltip}>{text}</span>
{/snippet}
{#snippet toggleSwitch(on: boolean, set: () => void, label: string)}
  <button type="button" role="switch" aria-checked={on} aria-label={label} onclick={set} class="relative shrink-0 w-9 h-5 rounded-full transition-colors {on ? 'bg-yt-primary' : 'bg-yt-border'}">
    <span class="absolute top-[2px] left-[2px] w-4 h-4 rounded-full bg-white transition-transform duration-200 {on ? 'translate-x-4' : ''}"></span>
  </button>
{/snippet}
{#snippet ffmpegChip()}
  <span class="ml-auto inline-flex items-center gap-1 px-1.5 py-0.5 rounded-full bg-yt-warning/10 text-yt-warning text-[9px] font-semibold uppercase tracking-wide"><span class="material-symbols-outlined text-[11px]">bolt</span>ffmpeg</span>
{/snippet}

<button type="button" aria-label={t("download.advClose")} class="fixed inset-0 z-40 bg-black/40 cursor-default animate-scrim-in" onclick={onClose}></button>

<aside class="fixed top-0 right-0 h-screen w-[400px] max-w-[88vw] z-50 bg-yt-surface border-l border-yt-border shadow-2xl flex flex-col animate-drawer-in">
  <header data-tauri-drag-region class="shrink-0 flex items-center gap-2.5 px-4 h-14 border-b border-yt-border">
    <span class="material-symbols-outlined text-yt-primary text-[20px]">tune</span>
    <h2 class="text-sm font-semibold text-yt-text">{t("download.advanced")}</h2>
    {#if activeAdvancedCount > 0}
      <span class="inline-flex items-center justify-center h-4 px-1.5 rounded-full bg-yt-primary/15 text-yt-primary text-[10px] font-semibold">{activeAdvancedCount}</span>
    {/if}
    <div class="ml-auto flex items-center gap-1">
      {#if activeAdvancedCount > 0}
        <button type="button" onclick={onReset} class="text-xs font-medium text-yt-text-secondary hover:text-yt-error transition-colors px-2 py-1 rounded">{t("download.advReset")}</button>
      {/if}
      <button type="button" aria-label={t("download.advClose")} onclick={onClose} class="w-7 h-7 rounded-md hover:bg-yt-highlight flex items-center justify-center text-yt-text-secondary">
        <span class="material-symbols-outlined text-[18px]">close</span>
      </button>
    </div>
  </header>

  <div class="flex-1 overflow-y-auto px-4 py-3.5 space-y-3 text-xs">
    <section class="bg-yt-bg border border-yt-border rounded-lg overflow-hidden">
      <div class="flex items-center gap-2 px-3.5 h-10 border-b border-yt-border/50">
        <span class="material-symbols-outlined text-[18px] text-yt-text-secondary">subtitles</span>
        <span class="text-xs font-semibold text-yt-text">{t("download.advSubsHeader")}</span>
      </div>
      <div class="p-3.5 space-y-2.5">
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advWriteSubs"), t("download.advWriteSubsHelp"))}
          {@render toggleSwitch(advanced.writeSubs, () => { advanced.writeSubs = !advanced.writeSubs; onSave() }, t("download.advWriteSubs"))}
        </div>
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advWriteAutoSubs"), t("download.advWriteAutoSubsHelp"))}
          {@render toggleSwitch(advanced.writeAutoSubs, () => { advanced.writeAutoSubs = !advanced.writeAutoSubs; onSave() }, t("download.advWriteAutoSubs"))}
        </div>
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advEmbedSubs"), t("download.advEmbedSubsHelp"))}
          {@render toggleSwitch(advanced.embedSubs, () => { advanced.embedSubs = !advanced.embedSubs; onSave() }, t("download.advEmbedSubs"))}
        </div>
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advSubLangs"), t("download.advSubLangsHelp"))}
          <input type="text" bind:value={advanced.subLangs} oninput={() => saveAdvancedText("subLangs")} placeholder={t("download.advSubLangsPlaceholder")} class="{txtCls} {advanced.subLangs.trim() !== '' && !validateAdvancedField('subLangs', advanced.subLangs) ? 'border-yt-error' : ''}" />
        </div>
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advConvertSubs"), t("download.advConvertSubsHelp"))}
          <select bind:value={advanced.convertSubs} onchange={onSave} class={selCls}>
            {#each SUB_CONVERT_FORMATS as f}
              <option value={f}>{f === "" ? t("settings.none") : f.toUpperCase()}</option>
            {/each}
          </select>
        </div>
      </div>
    </section>

    <section class="bg-yt-bg border border-yt-border rounded-lg overflow-hidden">
      <div class="flex items-center gap-2 px-3.5 h-10 border-b border-yt-border/50">
        <span class="material-symbols-outlined text-[18px] text-yt-text-secondary">shield</span>
        <span class="text-xs font-semibold text-yt-text">{t("download.advSbHeader")}</span>
        {@render ffmpegChip()}
      </div>
      <div class="p-3.5 space-y-2.5">
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advSbMode"), t("download.advSbModeHelp"))}
          <select bind:value={advanced.sponsorblockMode} onchange={onSave} class={selCls}>
            <option value="off">{t("download.advSbOff")}</option>
            <option value="mark">{t("download.advSbMark")}</option>
            <option value="remove">{t("download.advSbRemove")}</option>
          </select>
        </div>
        {#if advanced.sponsorblockMode === "remove"}
          <div class="flex items-start gap-1 text-[10px] text-yt-warning"><span class="material-symbols-outlined text-[12px] mt-px">warning</span><span>{t("download.advSbRemoveWarn")}</span></div>
        {/if}
        {#if advanced.sponsorblockMode !== "off"}
          <div class="space-y-1.5">
            {@render advLabel(t("download.advSbCategories"), t("download.advSbCategoriesHelp"))}
            <div class="flex flex-wrap gap-1.5">
              {#each SPONSORBLOCK_CATEGORIES as cat}
                <button type="button" onclick={() => toggleSponsorblockCategory(cat)} class="px-2 py-0.5 rounded-full text-[11px] border transition-colors {advanced.sponsorblockCategories.includes(cat) ? 'bg-yt-primary/10 border-yt-primary/40 text-yt-primary' : 'border-yt-border text-yt-text-secondary hover:border-yt-primary/40'}">{cat}</button>
              {/each}
            </div>
          </div>
        {/if}
      </div>
    </section>

    <section class="bg-yt-bg border border-yt-border rounded-lg overflow-hidden">
      <div class="flex items-center gap-2 px-3.5 h-10 border-b border-yt-border/50">
        <span class="material-symbols-outlined text-[18px] text-yt-text-secondary">sell</span>
        <span class="text-xs font-semibold text-yt-text">{t("download.advEmbedHeader")}</span>
        {@render ffmpegChip()}
      </div>
      <div class="p-3.5 space-y-2.5">
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advEmbedThumbnail"), t("download.advEmbedThumbnailHelp"))}
          {@render toggleSwitch(advanced.embedThumbnail, () => { advanced.embedThumbnail = !advanced.embedThumbnail; onSave() }, t("download.advEmbedThumbnail"))}
        </div>
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advEmbedMetadata"), t("download.advEmbedMetadataHelp"))}
          {@render toggleSwitch(advanced.embedMetadata, () => { advanced.embedMetadata = !advanced.embedMetadata; onSave() }, t("download.advEmbedMetadata"))}
        </div>
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advEmbedChapters"), t("download.advEmbedChaptersHelp"))}
          {@render toggleSwitch(advanced.embedChapters, () => { advanced.embedChapters = !advanced.embedChapters; onSave() }, t("download.advEmbedChapters"))}
        </div>
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advWriteThumbnail"), t("download.advWriteThumbnailHelp"))}
          {@render toggleSwitch(advanced.writeThumbnail, () => { advanced.writeThumbnail = !advanced.writeThumbnail; onSave() }, t("download.advWriteThumbnail"))}
        </div>
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advWriteInfoJson"), t("download.advWriteInfoJsonHelp"))}
          {@render toggleSwitch(advanced.writeInfoJson, () => { advanced.writeInfoJson = !advanced.writeInfoJson; onSave() }, t("download.advWriteInfoJson"))}
        </div>
      </div>
    </section>

    <section class="bg-yt-bg border border-yt-border rounded-lg overflow-hidden">
      <div class="flex items-center gap-2 px-3.5 h-10 border-b border-yt-border/50">
        <span class="material-symbols-outlined text-[18px] text-yt-text-secondary">movie</span>
        <span class="text-xs font-semibold text-yt-text">{t("download.advFormatHeader")}</span>
      </div>
      <div class="p-3.5 space-y-2.5">
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advVideoCodec"), t("download.advVideoCodecHelp"))}
          <select bind:value={advanced.videoCodec} onchange={onSave} class={selCls}>
            <option value="auto">{t("download.advCodecAuto")}</option>
            <option value="av01">AV1</option>
            <option value="vp9">VP9</option>
            <option value="h264">H.264</option>
          </select>
        </div>
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advLimitRate"), t("download.advLimitRateHelp"))}
          <input type="text" bind:value={advanced.limitRate} oninput={() => saveAdvancedText("limitRate")} placeholder="1M" class="{numCls} {advanced.limitRate.trim() !== '' && !validateAdvancedField('limitRate', advanced.limitRate) ? 'border-yt-error' : ''}" />
        </div>
        {#if advanced.videoCodec !== "auto"}
          <p class="text-[10px] text-yt-text-secondary/70 leading-snug">{t("download.advCodecHint")}</p>
        {/if}
      </div>
    </section>

    <section class="bg-yt-bg border border-yt-border rounded-lg overflow-hidden">
      <div class="flex items-center gap-2 px-3.5 h-10 border-b border-yt-border/50">
        <span class="material-symbols-outlined text-[18px] text-yt-text-secondary">lan</span>
        <span class="text-xs font-semibold text-yt-text">{t("download.advNetHeader")}</span>
      </div>
      <div class="p-3.5 space-y-2.5">
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advConcurrentFragments"), t("download.advConcurrentFragmentsHelp"))}
          <input type="number" min="1" max="16" value={advanced.concurrentFragments} oninput={(e) => setConcurrentFragments(e.currentTarget.value)} class={numCls} />
        </div>
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advRetries"), t("download.advRetriesHelp"))}
          <input type="number" min="0" max="100" value={advanced.retries ?? ""} oninput={(e) => setRetries(e.currentTarget.value)} class={numCls} />
        </div>
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advSleepInterval"), t("download.advSleepIntervalHelp"))}
          <input type="number" min="0" max={MAX_SLEEP_INTERVAL} value={advanced.sleepInterval} oninput={(e) => setSleepInterval(e.currentTarget.value)} class={numCls} />
        </div>
      </div>
    </section>

    <section class="bg-yt-bg border border-yt-border rounded-lg overflow-hidden">
      <div class="flex items-center gap-2 px-3.5 h-10 border-b border-yt-border/50">
        <span class="material-symbols-outlined text-[18px] text-yt-text-secondary">inventory_2</span>
        <span class="text-xs font-semibold text-yt-text">{t("download.advContainerHeader")}</span>
      </div>
      <div class="p-3.5 space-y-2.5">
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advMergeFormat"), t("download.advMergeFormatHelp"))}
          <select bind:value={advanced.mergeOutputFormat} onchange={onSave} class={selCls}>
            {#each CONTAINER_FORMATS as f}
              <option value={f}>{f === "" ? t("download.advCodecAuto") : f.toUpperCase()}</option>
            {/each}
          </select>
        </div>
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advRemuxVideo"), t("download.advRemuxVideoHelp"))}
          <select bind:value={advanced.remuxVideo} onchange={onSave} class={selCls}>
            {#each CONTAINER_FORMATS as f}
              <option value={f}>{f === "" ? t("settings.none") : f.toUpperCase()}</option>
            {/each}
          </select>
        </div>
      </div>
    </section>

    <section class="bg-yt-bg border border-yt-border rounded-lg overflow-hidden">
      <div class="flex items-center gap-2 px-3.5 h-10 border-b border-yt-border/50">
        <span class="material-symbols-outlined text-[18px] text-yt-text-secondary">content_cut</span>
        <span class="text-xs font-semibold text-yt-text">{t("download.advSectionsHeader")}</span>
        {@render ffmpegChip()}
      </div>
      <div class="p-3.5 space-y-2.5">
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advDownloadSections"), t("download.advDownloadSectionsHelp"))}
          <input type="text" bind:value={advanced.downloadSections} oninput={() => saveAdvancedText("downloadSections")} placeholder={t("download.advDownloadSectionsPlaceholder")} class="{txtCls} {advanced.downloadSections.trim() !== '' && !validateAdvancedField('downloadSections', advanced.downloadSections) ? 'border-yt-error' : ''}" />
        </div>
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advSplitChapters"), t("download.advSplitChaptersHelp"))}
          {@render toggleSwitch(advanced.splitChapters, () => { advanced.splitChapters = !advanced.splitChapters; onSave() }, t("download.advSplitChapters"))}
        </div>
      </div>
    </section>

    <section class="bg-yt-bg border border-yt-border rounded-lg overflow-hidden">
      <div class="flex items-center gap-2 px-3.5 h-10 border-b border-yt-border/50">
        <span class="material-symbols-outlined text-[18px] text-yt-text-secondary">hub</span>
        <span class="text-xs font-semibold text-yt-text">{t("download.advMiscHeader")}</span>
      </div>
      <div class="p-3.5 space-y-2.5">
        <div class="space-y-1.5">
          {@render advLabel(t("download.advProxy"), t("download.advProxyHelp"))}
          <input type="text" bind:value={advanced.proxy} oninput={() => saveAdvancedText("proxy")} placeholder="http://127.0.0.1:8080" class="w-full bg-yt-bg border border-yt-border rounded px-2 py-1 text-xs text-yt-text focus:ring-1 focus:ring-yt-primary focus:outline-none {advanced.proxy.trim() !== '' && !validateAdvancedField('proxy', advanced.proxy) ? 'border-yt-error' : ''}" />
        </div>
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advNoMtime"), t("download.advNoMtimeHelp"))}
          {@render toggleSwitch(advanced.noMtime, () => { advanced.noMtime = !advanced.noMtime; onSave() }, t("download.advNoMtime"))}
        </div>
        <div class="flex items-center justify-between gap-3 min-h-[28px]">
          {@render advLabel(t("download.advRestrictFilenames"), t("download.advRestrictFilenamesHelp"))}
          {@render toggleSwitch(advanced.restrictFilenames, () => { advanced.restrictFilenames = !advanced.restrictFilenames; onSave() }, t("download.advRestrictFilenames"))}
        </div>
      </div>
    </section>
  </div>

  <footer class="shrink-0 flex items-center gap-3 px-4 h-14 border-t border-yt-border">
    <button type="button" onclick={onReset} class="px-3 py-2 rounded-md text-xs font-medium text-yt-text-secondary hover:bg-yt-highlight transition-colors">{t("download.advReset")}</button>
    <button type="button" onclick={onClose} class="ml-auto px-5 h-9 rounded-md bg-yt-primary hover:bg-yt-primary-hover text-white text-sm font-medium transition-colors">{t("download.advDone")}</button>
  </footer>
</aside>

<style>
  @keyframes drawer-in {
    from { opacity: 0; transform: translateX(16px); }
    to { opacity: 1; transform: translateX(0); }
  }
  .animate-drawer-in {
    animation: drawer-in 0.22s cubic-bezier(0.16, 1, 0.3, 1);
  }
  @keyframes scrim-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }
  .animate-scrim-in {
    animation: scrim-in 0.2s ease-out;
  }
</style>
