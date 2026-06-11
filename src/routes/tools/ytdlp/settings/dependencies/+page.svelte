<script lang="ts">
  import { commands } from "$lib/bindings"
  import type { FullDependencyStatus, DepInstallEvent, AppSettings } from "$lib/bindings"
  import { defaultAdvancedOptions } from "$lib/advanced"
  import { onMount, onDestroy } from "svelte"
  import { listen } from "@tauri-apps/api/event"
  import { revealItemInDir } from "@tauri-apps/plugin-opener"
  import { t } from "$lib/i18n/index.svelte"
  import { extractError } from "$lib/utils/errors"

  let settings = $state<AppSettings>({
    downloadPath: "",
    defaultQuality: "1080p",
    maxConcurrent: 2,
    filenameTemplate: "%(title)s.%(ext)s",
    cookieBrowser: null,
    autoUpdateYtdlp: true,
    useAdvancedTemplate: false,
    templateUploaderFolder: false,
    templateUploadDate: false,
    templateVideoId: false,
    language: null,
    theme: null,
    minimizeToTray: null,
    depMode: "hybrid",
    depOverrides: {},
    advanced: defaultAdvancedOptions(),
    setupCompleted: true,
  })

  let loading = $state(true)

  // Normalize the stored mode for display: the legacy "external" value is shown
  // as "bundled" so the matching card lights up.
  let activeDepMode = $derived(
    settings.depMode === "bundled" || settings.depMode === "external" ? "bundled" : "hybrid"
  )
  // Both hybrid and bundled manage app-side binaries, so install/update stays available.
  let usesAppBin = $derived(true)

  // Dependency management state
  let depStatus = $state<FullDependencyStatus | null>(null)
  let depLoading = $state(true)
  // Per-dependency in-flight flags so concurrent installs/updates each track and
  // disable only their own button. A single `string | null` would re-enable the
  // first button (and drop its spinner) the moment a second operation started.
  let updatingDeps = $state<Record<string, boolean>>({})
  let installingDeps = $state<Record<string, boolean>>({})
  let installingAll = $state(false)
  // Per-dependency action result so two operations in flight at once don't clobber
  // each other's message. The "all" key holds the Install All result.
  let depActionResults = $state<Record<string, { success: boolean, message: string }>>({})
  let installProgress = $state<Record<string, { stage: string, percent: number, message: string | null }>>({})
  // Inline note shown when a user taps a source that isn't available for a dep.
  let sourceHint = $state<{ dep: string, message: string } | null>(null)
  // In-flight loadDepStatus count: only stop the spinner when the last call resolves.
  let depLoadCount = $state(0)

  // Install handlers unlisten in their finally blocks, but an install can outlive
  // this page — navigating away mid-install would leave the listener mutating a
  // destroyed component's state. Track every live unlisten so onDestroy can drop them.
  const activeUnlistens = new Set<() => void>()

  onDestroy(() => {
    for (const fn of activeUnlistens) fn()
    activeUnlistens.clear()
  })

  function setResult(dep: string, success: boolean, message: string) {
    depActionResults = { ...depActionResults, [dep]: { success, message } }
  }

  function clearResult(dep: string) {
    const next = { ...depActionResults }
    delete next[dep]
    depActionResults = next
  }

  async function loadDepStatus(force = false) {
    depLoadCount++
    depLoading = true
    try {
      const result = await commands.checkFullDependencies(force)
      if (result.status === "ok") {
        depStatus = result.data
      }
    } catch (e) {
      console.error("Failed to load dep status:", e)
    } finally {
      depLoadCount--
      if (depLoadCount === 0) depLoading = false
    }
  }

  async function handleInstallDep(depName: string) {
    installingDeps = { ...installingDeps, [depName]: true }
    clearResult(depName)

    let unlistenFn: (() => void) | null = null
    try {
      unlistenFn = await listen("dep-install-event", (event: any) => {
        const data = event.payload as DepInstallEvent
        // Immediately mark dep as installed when Completing stage is received
        if (data.stage === "Completing" && data.depName === depName && depStatus) {
          const depKey = depName === "yt-dlp" ? "ytdlp" : depName as "ffmpeg" | "deno"
          depStatus = {
            ...depStatus,
            [depKey]: {
              ...depStatus[depKey],
              installed: true,
              source: "AppManaged",
            },
          }
        }
      })
      activeUnlistens.add(unlistenFn)
    } catch (e) {
      console.error("Failed to listen for dep install events:", e)
    }

    try {
      const result = await commands.installDependency(depName)
      if (result.status === "ok") {
        setResult(depName, true, result.data)
      } else {
        setResult(depName, false, extractError(result.error))
      }
    } catch (e: any) {
      setResult(depName, false, e?.message || String(e))
    } finally {
      installingDeps = { ...installingDeps, [depName]: false }
      if (unlistenFn) {
        unlistenFn()
        activeUnlistens.delete(unlistenFn)
      }
      await loadDepStatus(true)
    }
  }

  async function handleInstallAll() {
    installingAll = true
    clearResult("all")
    installProgress = {}

    let unlistenFn: (() => void) | null = null
    try {
      unlistenFn = await listen("dep-install-event", (event: any) => {
        const data = event.payload as DepInstallEvent
        installProgress[data.depName] = {
          stage: data.stage,
          percent: data.percent,
          message: data.message ?? null,
        }
        installProgress = { ...installProgress }

        // Immediately mark dep as installed when Completing stage is received
        if (data.stage === "Completing" && depStatus) {
          const depKey = data.depName === "yt-dlp" ? "ytdlp" : data.depName as "ffmpeg" | "deno"
          depStatus = {
            ...depStatus,
            [depKey]: {
              ...depStatus[depKey],
              installed: true,
              source: "AppManaged",
            },
          }
        }
      })
      activeUnlistens.add(unlistenFn)
    } catch (e) {
      console.error("Failed to listen for dep install events:", e)
    }

    try {
      const result = await commands.installAllDependencies()
      if (result.status === "ok") {
        const failures = result.data.filter(r => r.includes("FAILED"))
        if (failures.length > 0) {
          setResult("all", false, failures.join("\n"))
        } else {
          setResult("all", true, t("layout.installSuccess"))
        }
      } else {
        setResult("all", false, extractError(result.error))
      }
    } catch (e: any) {
      setResult("all", false, e?.message || String(e))
    } finally {
      installingAll = false
      if (unlistenFn) {
        unlistenFn()
        activeUnlistens.delete(unlistenFn)
      }
      installProgress = {}
      await loadDepStatus(true)
    }
  }

  async function handleUpdateDep(depName: string) {
    updatingDeps = { ...updatingDeps, [depName]: true }
    clearResult(depName)
    try {
      const result = await commands.updateDependency(depName)
      if (result.status === "ok") {
        setResult(depName, true, result.data)
      } else {
        setResult(depName, false, extractError(result.error))
      }
    } catch (e: any) {
      setResult(depName, false, e?.message || String(e))
    } finally {
      updatingDeps = { ...updatingDeps, [depName]: false }
      await loadDepStatus(true)
    }
  }

  let saveError = $state<string | null>(null)

  // tauri-specta Results don't throw: check the status so a rejected save is surfaced and
  // the mode/source picker doesn't show a choice the backend never persisted.
  async function autoSave(): Promise<boolean> {
    saveError = null
    try {
      const result = await commands.updateSettings(settings)
      if (result.status === "error") {
        saveError = t("settings.saveFailed", { error: extractError(result.error) })
        return false
      }
      return true
    } catch (e) {
      console.error("Failed to save settings:", e)
      saveError = t("settings.saveFailed", { error: extractError(e) })
      return false
    }
  }

  async function handleDepModeChange(mode: string) {
    const previousMode = settings.depMode
    const previousOverrides = settings.depOverrides
    settings.depMode = mode
    // The new global mode is authoritative; drop any per-item overrides so a
    // lingering pick doesn't contradict it (e.g. a "system" pin under bundled).
    settings.depOverrides = {}
    if (!(await autoSave())) {
      settings.depMode = previousMode
      settings.depOverrides = previousOverrides
      return
    }
    await loadDepStatus(true)
  }

  // Per-dependency source override (hybrid mode). Pins which copy — the app's
  // bundled one or the system-PATH one — yt-dlp/ffmpeg/deno actually run from.
  // Tapping a source that isn't installed can't switch to it, so we explain why.
  async function handleSourceToggle(
    depKey: string,
    source: "appManaged" | "systemPath",
    available: boolean | undefined,
  ) {
    if (!available) {
      sourceHint = {
        dep: depKey,
        message: source === "systemPath"
          ? t("settings.sourceUnavailableSystem")
          : t("settings.sourceUnavailableBundled"),
      }
      return
    }
    sourceHint = null
    const previousOverrides = settings.depOverrides
    settings.depOverrides = { ...(settings.depOverrides ?? {}), [depKey]: source }
    if (!(await autoSave())) {
      settings.depOverrides = previousOverrides
      return
    }
    await loadDepStatus(true)
  }

  function activeSource(depKey: string, info: { source: string }): string | undefined {
    const override = settings.depOverrides?.[depKey]
    if (override) return override
    if (info.source === "AppManaged") return "appManaged"
    if (info.source === "SystemPath") return "systemPath"
    return undefined
  }

  async function handleRevealPath(path: string) {
    try {
      await revealItemInDir(path)
    } catch (e) {
      console.error("Failed to reveal path:", e)
    }
  }

  let missingCount = $derived(
    depStatus
      ? [depStatus.ytdlp, depStatus.ffmpeg, depStatus.deno].filter(d => !d.installed).length
      : 0
  )

  onMount(async () => {
    try {
      const r = await commands.getSettings()
      if (r.status === "ok") settings = r.data
    } catch (e) { console.error("Failed to load settings:", e) }
    loading = false
    loadDepStatus()
  })
</script>

{#if loading}
  <div class="flex justify-center py-16">
    <span class="material-symbols-outlined text-yt-primary text-3xl animate-spin">progress_activity</span>
  </div>
{:else}
  <div class="max-w-5xl mx-auto px-8 py-8 space-y-10">

    {#if saveError}
      <div class="flex items-center gap-2 text-xs text-yt-error bg-yt-error/10 border border-yt-error/30 rounded-md px-3 py-2">
        <span class="material-symbols-outlined text-[16px]">error</span>
        <span>{saveError}</span>
      </div>
    {/if}

    <!-- Dependency Mode -->
    <section>
      <h3 class="text-xs font-semibold text-yt-text-secondary uppercase tracking-wider mb-4 px-1">{t("settings.depMode")}</h3>
      <p class="text-xs text-yt-text-secondary mb-4 px-1">{t("settings.depModeLabel")}</p>
      <div class="grid grid-cols-2 gap-3">
        <!-- Hybrid Mode (recommended) -->
        <button
          onclick={() => handleDepModeChange("hybrid")}
          class="text-left p-4 rounded-lg border-2 transition-all {activeDepMode === 'hybrid'
            ? 'border-yt-primary bg-yt-primary/5 ring-1 ring-yt-primary'
            : 'border-yt-border bg-yt-surface hover:bg-yt-highlight'}"
        >
          <div class="flex items-center gap-2 mb-2">
            <span class="material-symbols-outlined text-[20px] {activeDepMode === 'hybrid' ? 'text-yt-primary' : 'text-yt-text-secondary'}">auto_awesome</span>
            <span class="text-sm font-semibold text-yt-text">{t("settings.depModeHybrid")}</span>
          </div>
          <p class="text-[11px] text-yt-text-secondary leading-relaxed">{t("settings.depModeHybridDesc")}</p>
        </button>

        <!-- Bundled Mode -->
        <button
          onclick={() => handleDepModeChange("bundled")}
          class="text-left p-4 rounded-lg border-2 transition-all {activeDepMode === 'bundled'
            ? 'border-yt-primary bg-yt-primary/5 ring-1 ring-yt-primary'
            : 'border-yt-border bg-yt-surface hover:bg-yt-highlight'}"
        >
          <div class="flex items-center gap-2 mb-2">
            <span class="material-symbols-outlined text-[20px] {activeDepMode === 'bundled' ? 'text-yt-primary' : 'text-yt-text-secondary'}">package_2</span>
            <span class="text-sm font-semibold text-yt-text">{t("settings.depModeBundled")}</span>
          </div>
          <p class="text-[11px] text-yt-text-secondary leading-relaxed">{t("settings.depModeBundledDesc")}</p>
        </button>
      </div>
    </section>

    <!-- Dependencies Status -->
    <section>
      <div class="flex items-center justify-between mb-4 px-1">
        <h3 class="text-xs font-semibold text-yt-text-secondary uppercase tracking-wider">{t("settings.dependencies")}</h3>
        {#if usesAppBin && missingCount > 0 && !installingAll}
          <button
            onclick={handleInstallAll}
            class="px-3 py-1.5 text-xs font-medium bg-yt-primary hover:bg-yt-primary-hover text-white rounded-md transition-colors flex items-center gap-1"
          >
            <span class="material-symbols-outlined text-[14px]">download</span>
            {t("settings.installAll")}
          </button>
        {/if}
      </div>
      <div class="bg-yt-surface border border-yt-border rounded-lg divide-y divide-yt-border/50 overflow-hidden">
        {#if depLoading}
          <div class="p-4 flex justify-center">
            <span class="material-symbols-outlined text-yt-primary text-xl animate-spin">progress_activity</span>
          </div>
        {:else if depStatus}
          {#each [
            { key: "yt-dlp", info: depStatus.ytdlp },
            { key: "ffmpeg", info: depStatus.ffmpeg },
            { key: "deno", info: depStatus.deno },
          ] as dep}
            <div class="p-4 flex items-center justify-between gap-4">
              <div class="flex items-center gap-3 min-w-0 flex-1">
                <span class="material-symbols-outlined text-[20px] {dep.info.installed ? 'text-yt-success' : 'text-yt-error'}">
                  {dep.info.installed ? "check_circle" : "cancel"}
                </span>
                <div class="min-w-0 flex-1">
                  <p class="text-sm font-medium text-yt-text">{dep.key}</p>
                  <p class="text-[11px] text-yt-text-secondary truncate">
                    {#if dep.info.installed}
                      {dep.info.version || t("layout.installed")}
                      <span class="ml-1 text-[10px] px-1.5 py-0.5 rounded bg-yt-highlight text-yt-text-muted">
                        {dep.info.source === "AppManaged" ? t("settings.appManaged") : t("settings.systemPath")}
                      </span>
                    {:else}
                      {t("settings.notInstalled")}
                    {/if}
                  </p>
                  {#if dep.info.installed && dep.info.path}
                    <button
                      onclick={() => handleRevealPath(dep.info.path!)}
                      class="mt-1 flex items-center gap-1 text-[10px] text-yt-text-muted hover:text-yt-primary transition-colors max-w-full"
                      title={dep.info.path}
                      aria-label={t("settings.openLocation")}
                    >
                      <span class="material-symbols-outlined text-[12px] shrink-0">folder_open</span>
                      <span class="font-mono truncate">{dep.info.path}</span>
                    </button>
                  {/if}
                  <!-- Install progress -->
                  {#if installingAll && installProgress[dep.key]}
                    <div class="mt-2">
                      <div class="h-1 bg-yt-border rounded-full overflow-hidden">
                        <div
                          class="h-full bg-yt-primary transition-all duration-300 rounded-full"
                          style="width: {installProgress[dep.key].percent}%"
                        ></div>
                      </div>
                      <p class="text-[9px] text-yt-text-muted mt-1">
                        {installProgress[dep.key].stage === "Downloading" ? t("layout.depDownloading") : ""}
                        {installProgress[dep.key].stage === "Extracting" ? t("layout.extracting") : ""}
                        {installProgress[dep.key].stage === "Verifying" ? t("layout.verifying") : ""}
                        {installProgress[dep.key].stage === "Completing" ? t("layout.installSuccess") : ""}
                        {installProgress[dep.key].stage === "Failed" ? t("layout.installFailed") : ""}
                        {installProgress[dep.key].percent > 0 ? ` ${installProgress[dep.key].percent.toFixed(0)}%` : ""}
                      </p>
                    </div>
                  {/if}
                  {#if sourceHint?.dep === dep.key}
                    <p class="mt-1.5 flex items-start gap-1 text-[10px] text-yt-warning">
                      <span class="material-symbols-outlined text-[12px] shrink-0">info</span>
                      <span>{sourceHint.message}</span>
                    </p>
                  {/if}
                </div>
              </div>
              <div class="flex items-center gap-2 shrink-0">
                {#if activeDepMode === "hybrid" && (dep.info.appAvailable || dep.info.systemAvailable)}
                  {@const active = activeSource(dep.key, dep.info)}
                  <div class="inline-flex rounded-md border border-yt-border overflow-hidden text-[11px]">
                    <button
                      onclick={() => handleSourceToggle(dep.key, "appManaged", dep.info.appAvailable)}
                      title={t("settings.appManaged")}
                      class="px-2.5 py-1 transition-colors {active === 'appManaged' ? 'bg-yt-primary text-white' : !dep.info.appAvailable ? 'text-yt-text-muted/40 hover:bg-yt-highlight/40' : 'text-yt-text-secondary hover:bg-yt-highlight'}"
                    >
                      {t("settings.appManaged")}
                    </button>
                    <button
                      onclick={() => handleSourceToggle(dep.key, "systemPath", dep.info.systemAvailable)}
                      title={t("settings.systemPath")}
                      class="px-2.5 py-1 border-l border-yt-border transition-colors {active === 'systemPath' ? 'bg-yt-primary text-white' : !dep.info.systemAvailable ? 'text-yt-text-muted/40 hover:bg-yt-highlight/40' : 'text-yt-text-secondary hover:bg-yt-highlight'}"
                    >
                      {t("settings.systemPath")}
                    </button>
                  </div>
                {/if}
                {#if usesAppBin}
                  {#if !dep.info.installed}
                    <button
                      onclick={() => handleInstallDep(dep.key)}
                      disabled={installingDeps[dep.key] || installingAll}
                      class="px-3 py-1.5 text-xs font-medium bg-yt-primary hover:bg-yt-primary-hover text-white rounded-md transition-colors disabled:opacity-50 flex items-center gap-1"
                    >
                      {#if installingDeps[dep.key]}
                        <span class="material-symbols-outlined text-[14px] animate-spin">progress_activity</span>
                      {/if}
                      {t("settings.install")}
                    </button>
                  {:else if dep.info.source === "AppManaged"}
                    <button
                      onclick={() => handleUpdateDep(dep.key)}
                      disabled={updatingDeps[dep.key] || installingAll}
                      class="px-3 py-1.5 text-xs font-medium bg-yt-highlight hover:bg-yt-border text-yt-text rounded-md transition-colors disabled:opacity-50 flex items-center gap-1"
                    >
                      {#if updatingDeps[dep.key]}
                        <span class="material-symbols-outlined text-[14px] animate-spin">progress_activity</span>
                        {t("settings.updating")}
                      {:else}
                        {t("settings.update")}
                      {/if}
                    </button>
                  {/if}
                {/if}
              </div>
            </div>
          {/each}
          {#each Object.entries(depActionResults) as [dep, res] (dep)}
            <div class="p-3 {res.success ? 'bg-green-500/5' : 'bg-red-500/5'}">
              <p class="text-xs {res.success ? 'text-yt-success' : 'text-red-400'}">
                {dep === "all" ? "" : `${dep}: `}{res.message}
              </p>
            </div>
          {/each}
        {/if}
      </div>
    </section>

  </div>
{/if}
