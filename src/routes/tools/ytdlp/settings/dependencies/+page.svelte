<script lang="ts">
  import { commands } from "$lib/bindings"
  import type { FullDependencyStatus, DepInstallEvent } from "$lib/bindings"
  import { onMount } from "svelte"
  import { listen } from "@tauri-apps/api/event"
  import { t } from "$lib/i18n/index.svelte"

  let settings = $state({
    downloadPath: "",
    defaultQuality: "1080p",
    maxConcurrent: 3,
    filenameTemplate: "%(title)s.%(ext)s",
    cookieBrowser: null as string | null,
    autoUpdateYtdlp: true,
    useAdvancedTemplate: false,
    templateUploaderFolder: false,
    templateUploadDate: false,
    templateVideoId: false,
    language: null as string | null,
    theme: null as string | null,
    minimizeToTray: null as boolean | null,
    depMode: "external",
  })

  let loading = $state(true)

  // Dependency management state
  let depStatus = $state<FullDependencyStatus | null>(null)
  let depLoading = $state(true)
  let updatingDep = $state<string | null>(null)
  let installingDep = $state<string | null>(null)
  let installingAll = $state(false)
  let depActionResult = $state<{ dep: string, success: boolean, message: string } | null>(null)
  let installProgress = $state<Record<string, { stage: string, percent: number, message: string | null }>>({})

  async function loadDepStatus(force = false) {
    depLoading = true
    try {
      const result = await commands.checkFullDependencies(force)
      if (result.status === "ok") {
        depStatus = result.data
      }
    } catch (e) { console.error("Failed to load dep status:", e) }
    depLoading = false
  }

  async function handleInstallDep(depName: string) {
    installingDep = depName
    depActionResult = null

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
    } catch (e) {
      console.error("Failed to listen for dep install events:", e)
    }

    try {
      const result = await commands.installDependency(depName)
      if (result.status === "ok") {
        depActionResult = { dep: depName, success: true, message: result.data }
      } else {
        depActionResult = { dep: depName, success: false, message: Object.values(result.error)[0] as string }
      }
    } catch (e: any) {
      depActionResult = { dep: depName, success: false, message: e?.message || String(e) }
    } finally {
      installingDep = null
      if (unlistenFn) unlistenFn()
      await loadDepStatus(true)
    }
  }

  async function handleInstallAll() {
    installingAll = true
    depActionResult = null
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
    } catch (e) {
      console.error("Failed to listen for dep install events:", e)
    }

    try {
      const result = await commands.installAllDependencies()
      if (result.status === "ok") {
        const failures = result.data.filter(r => r.includes("FAILED"))
        if (failures.length > 0) {
          depActionResult = { dep: "all", success: false, message: failures.join("\n") }
        } else {
          depActionResult = { dep: "all", success: true, message: t("layout.installSuccess") }
        }
      } else {
        depActionResult = { dep: "all", success: false, message: Object.values(result.error)[0] as string }
      }
    } catch (e: any) {
      depActionResult = { dep: "all", success: false, message: e?.message || String(e) }
    } finally {
      installingAll = false
      if (unlistenFn) unlistenFn()
      installProgress = {}
      await loadDepStatus(true)
    }
  }

  async function handleUpdateDep(depName: string) {
    updatingDep = depName
    depActionResult = null
    try {
      const result = await commands.updateDependency(depName)
      if (result.status === "ok") {
        depActionResult = { dep: depName, success: true, message: result.data }
      } else {
        depActionResult = { dep: depName, success: false, message: Object.values(result.error)[0] as string }
      }
    } catch (e: any) {
      depActionResult = { dep: depName, success: false, message: e?.message || String(e) }
    } finally {
      updatingDep = null
      await loadDepStatus(true)
    }
  }

  async function autoSave() {
    try { await commands.updateSettings(settings) }
    catch (e) { console.error("Failed to save settings:", e) }
  }

  async function handleDepModeChange(mode: string) {
    settings.depMode = mode
    await autoSave()
    await loadDepStatus(true)
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
  <div class="max-w-3xl mx-auto px-8 py-8 space-y-10">

    <!-- Dependency Mode -->
    <section>
      <h3 class="text-xs font-semibold text-yt-text-secondary uppercase tracking-wider mb-4 px-1">{t("settings.depMode")}</h3>
      <p class="text-xs text-yt-text-secondary mb-4 px-1">{t("settings.depModeLabel")}</p>
      <div class="grid grid-cols-2 gap-3">
        <!-- External Mode -->
        <button
          onclick={() => handleDepModeChange("external")}
          class="text-left p-4 rounded-lg border-2 transition-all {settings.depMode === 'external'
            ? 'border-yt-primary bg-yt-primary/5 ring-1 ring-yt-primary'
            : 'border-yt-border bg-yt-surface hover:bg-yt-highlight'}"
        >
          <div class="flex items-center gap-2 mb-2">
            <span class="material-symbols-outlined text-[20px] {settings.depMode === 'external' ? 'text-yt-primary' : 'text-yt-text-secondary'}">cloud_download</span>
            <span class="text-sm font-semibold text-yt-text">{t("settings.depModeExternal")}</span>
          </div>
          <p class="text-[11px] text-yt-text-secondary leading-relaxed">{t("settings.depModeExternalDesc")}</p>
        </button>

        <!-- System Mode -->
        <button
          onclick={() => handleDepModeChange("system")}
          class="text-left p-4 rounded-lg border-2 transition-all {settings.depMode === 'system'
            ? 'border-yt-primary bg-yt-primary/5 ring-1 ring-yt-primary'
            : 'border-yt-border bg-yt-surface hover:bg-yt-highlight'}"
        >
          <div class="flex items-center gap-2 mb-2">
            <span class="material-symbols-outlined text-[20px] {settings.depMode === 'system' ? 'text-yt-primary' : 'text-yt-text-secondary'}">terminal</span>
            <span class="text-sm font-semibold text-yt-text">{t("settings.depModeSystem")}</span>
          </div>
          <p class="text-[11px] text-yt-text-secondary leading-relaxed">{t("settings.depModeSystemDesc")}</p>
        </button>
      </div>
    </section>

    <!-- Dependencies Status -->
    <section>
      <div class="flex items-center justify-between mb-4 px-1">
        <h3 class="text-xs font-semibold text-yt-text-secondary uppercase tracking-wider">{t("settings.dependencies")}</h3>
        {#if settings.depMode === "external" && missingCount > 0 && !installingAll}
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
                </div>
              </div>
              <div class="flex gap-2 shrink-0">
                {#if settings.depMode === "external"}
                  {#if !dep.info.installed}
                    <button
                      onclick={() => handleInstallDep(dep.key)}
                      disabled={installingDep === dep.key || installingAll}
                      class="px-3 py-1.5 text-xs font-medium bg-yt-primary hover:bg-yt-primary-hover text-white rounded-md transition-colors disabled:opacity-50 flex items-center gap-1"
                    >
                      {#if installingDep === dep.key}
                        <span class="material-symbols-outlined text-[14px] animate-spin">progress_activity</span>
                      {/if}
                      {t("settings.install")}
                    </button>
                  {:else if dep.info.source === "AppManaged"}
                    <button
                      onclick={() => handleUpdateDep(dep.key)}
                      disabled={updatingDep === dep.key || installingAll}
                      class="px-3 py-1.5 text-xs font-medium bg-yt-highlight hover:bg-yt-border text-yt-text rounded-md transition-colors disabled:opacity-50 flex items-center gap-1"
                    >
                      {#if updatingDep === dep.key}
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
          {#if depActionResult}
            <div class="p-3 {depActionResult.success ? 'bg-green-500/5' : 'bg-red-500/5'}">
              <p class="text-xs {depActionResult.success ? 'text-yt-success' : 'text-red-400'}">
                {depActionResult.dep === "all" ? "" : `${depActionResult.dep}: `}{depActionResult.message}
              </p>
            </div>
          {/if}
        {/if}
      </div>
    </section>

  </div>
{/if}
