/** Built-in Client Plugin: configure default ACP Harnesses and open new sessions. */
import {
  addHarnessAsync, discoverHarnessCandidates, discoverHarnesses, fetchAcpRegistry,
  setDefaultHarness, selectedHarness, tokenizeHarnessArgs, upsertHarness, savedHarnesses,
} from './harnesses.js'
import { planHarnessRemoval, removeHarness } from './harness-removal.js'
import { scanHarnessCandidates } from './harness-discovery.js'
import { readAcpRegistrySnapshot } from './harness-registry.js'
import { stripVTControlCharacters } from 'node:util'

export const name = 'harness-view'
export const inject = ['tuiCommands', 'tuiOverlay', 'acpClient', 'acpSessionStatus', 'tuiSlots']

const ADD = ':add'
const REFRESH = ':refresh'
const MANUAL = ':manual'
const BACK = ':back'
const DOWNLOAD = ':download:'
const addOption = { value: ADD, label: '+ Add Harness…', description: 'Browse the ACP Registry and local programs' }
const manualOption = { value: MANUAL, label: 'Manual configuration…', description: 'Use an ACP command not listed here' }
const commandText = (entry) => [entry.resolvedCommand ?? entry.command, ...(entry.args ?? [])]
  .map((part) => /\s/.test(part) ? JSON.stringify(part) : part).join(' ')
const errorText = (error) => error instanceof Error ? error.message : String(error)
const recipeIdentity = (entry) => JSON.stringify([
  entry.id, entry.resolvedCommand ?? entry.command, entry.args ?? [],
  Object.entries(entry.env ?? {}).sort(([left], [right]) => left.localeCompare(right)),
  ...(entry.distribution?.type === 'binary'
    ? [entry.version, entry.distribution.target, entry.distribution.archive] : []),
])
const locallyAvailable = (entry) => ['Configured', 'Installed', 'Found locally'].includes(entry.status)
  || ['builtin', 'forced', 'path', 'managed'].includes(entry.source)

function findDescription(entry) {
  if (entry.status === 'Checking locally') return 'checking local installation…'
  if (entry.status === 'Available to install') return `install in Martty · ${entry.installPath ?? 'private bin directory'}`
  if (entry.status?.startsWith('Available via ')) return `${entry.status.toLowerCase()} · first launch may download packages`
  if (entry.status === 'Needs npx') return 'needs npx · install Node.js/npm'
  if (entry.status === 'Needs uvx') return 'needs uvx · install uv'
  if (entry.status === 'Not installed') return `not installed · install ${commandText(entry.install ?? {})}`
  if (entry.status === 'Configured') return `configured · ${commandText(entry)}`
  return `found locally · configure · ${commandText(entry)}`
}

export function apply(ctx, options = {}) {
  const settingsPath = options.settingsPath
  let registryPromise
  let registrySnapshot = options.registry ?? readAcpRegistrySnapshot({ ...options, settingsPath })
  let candidateSnapshot = []
  let flow = 0
  let disposed = false
  let operation
  let ownedOverlay
  let pendingFailure
  let commandRegistration
  const downloads = new Map()
  const configurationRevisions = new Map()
  const removing = new Set()
  let downloadSequence = 0
  let downloadRevision = 0
  let visibleDownload
  let downloadNotice
  const stopDownloadSlot = ctx.tuiSlots?.inject('conversation.input.dock', () => {
    downloadNotice = ctx.tuiSlots.register({ name: 'conversation.input.dock', id: 'harness-downloads', order: -20 }, [])
    return () => downloadNotice.dispose()
  })
  const runningRecipe = options.forcedHarness
  const isRunning = (entry) => {
    if (typeof ctx.acpClient?.hasAgent === 'function') return ctx.acpClient.hasAgent(entry)
    const child = ctx.acpClient?.child
    if (child?.exitCode != null || child?.signalCode != null) return false
    const live = ctx.acpClient?.command ? ctx.acpClient : runningRecipe
    return live != null && recipeIdentity({ id: '', command: entry.resolvedCommand ?? entry.command, args: entry.args, env: entry.env }) === recipeIdentity({
      id: '', command: live.command, args: live.args, env: live.env,
    })
  }
  const currentOption = (entry) => entry.id === selectedHarness(settingsPath)?.id ? { label: `${entry.label} (default)` } : {}
  const currentFirst = (left, right) => Number(right.id === selectedHarness(settingsPath)?.id) - Number(left.id === selectedHarness(settingsPath)?.id)
  const choices = () => discoverHarnesses(settingsPath, options).sort(currentFirst).map((entry) => ({
    value: entry.id, label: entry.label, description: `${entry.source} · ${commandText(entry)}`,
    ...currentOption(entry),
  }))
  const commandChoices = () => [...choices(), { ...addOption, value: 'add' }]
  const refreshCommandChoices = () => commandRegistration?.update({ input: {
    hint: '[id] [--new] | add | remove [id] | find [query]', options: commandChoices(),
  } })

  function openView(spec, handlers) {
    if (disposed) return
    ownedOverlay = ctx.tuiOverlay.openView(spec, handlers)
    return ownedOverlay
  }
  function openSelect(spec, handlers) {
    if (disposed) return
    ownedOverlay = ctx.tuiOverlay.openSelect(spec, handlers)
    return ownedOverlay
  }

  async function loadRegistry(refresh = false, signal) {
    if (options.registry !== undefined) return options.registry
    if (refresh) registryPromise = undefined
    if (registryPromise === undefined) {
      registryPromise = Promise.resolve().then(() => (options.fetchRegistry ?? fetchAcpRegistry)({ ...options, signal }))
    }
    const pending = registryPromise
    try {
      const registry = await pending
      if (!disposed) registrySnapshot = registry
      return registry
    } catch (error) {
      if (registryPromise === pending) registryPromise = undefined
      throw error
    }
  }

  function manual(id = 'local') {
    openView({ id: 'harness-manual', title: 'Manual ACP command · enter back', nodes: [{
      id: 'instructions', kind: 'markdown',
      text: `The command must start an ACP server on stdin/stdout.\n\n`
        + `\`/harness add ${id} --command <path> --arg <argument>\`\n\n`
        + 'Quote paths containing spaces. Repeat --arg for each argument. An ordinary agent CLI is not necessarily an ACP server.',
    }] }, { onSubmit: () => browse() })
  }

  function retryView(title, error, retry, query = '') {
    if (disposed) return
    if (ctx.tuiOverlay.active() !== null) {
      pendingFailure = { title, error, retry, query }
      return
    }
    const clean = (value) => stripVTControlCharacters(String(value))
      .replace(/[\x00-\x08\x0b-\x1f\x7f-\x9f]/g, '').trim()
    const raw = errorText(error)
    const marker = '\nAgent stderr:\n'
    const boundary = raw.indexOf(marker)
    const message = clean(error?.acpError?.message ?? (boundary < 0 ? raw : raw.slice(0, boundary)))
    const diagnostics = clean(error?.diagnostics ?? (boundary < 0 ? '' : raw.slice(boundary + marker.length)))
    const code = error?.acpError?.code ?? error?.code
    const context = [error?.method, code === undefined ? undefined : `Code: ${code}`].filter(Boolean).join(' · ')
    const nodes = [
      { id: 'reason', kind: 'notice', level: 'error', text: message || 'The Harness could not be started.' },
      ...(context ? [{ id: 'context', kind: 'notice', level: 'info', text: clean(context) }] : []),
      ...(error?.acpError?.data === undefined ? [] : [{
        id: 'error-data', kind: 'generic', title: 'Error data', body: clean(JSON.stringify(error.acpError.data, null, 2)),
      }]),
      ...(diagnostics ? [{ id: 'stderr', kind: 'generic', title: 'Agent stderr', body: diagnostics }] : []),
      { id: 'actions', kind: 'notice', level: 'info', text: 'Enter retries this operation. Esc closes; use /harness to choose another Harness.' },
    ]
    openView({ id: 'harness-setup-error', title: `${title} · enter retry`, nodes }, { onSubmit: retry })
  }

  const removalOptions = () => ({ ...options, isCurrent: isRunning })
  function removal(id, back = () => showHarnessPicker(id), selected = 'config') {
    try {
      if (!id) {
        const entries = savedHarnesses(settingsPath).map(entry => {
          try {
            planHarnessRemoval(settingsPath, entry.id, removalOptions())
            return { value: entry.id, label: entry.label, description: commandText(entry) }
          } catch (error) {
            return { value: entry.id, label: entry.label, description: errorText(error), disabled: true }
          }
        })
        if (!entries.length) return openView({ id: 'harness-remove-empty', title: 'Remove Harness', nodes: [{
          id: 'empty', kind: 'notice', level: 'info', text: 'No saved Harness configuration to remove.',
        }] })
        return openSelect({ id: 'harness-remove', title: 'Remove Harness · esc back',
          value: entries.find(entry => entry.value === selected)?.value ?? entries[0].value, options: entries }, {
          onCancel: back,
          onSubmit: target => removal(target, () => removal(undefined, back, target)),
        })
      }
      const plan = planHarnessRemoval(settingsPath, id, removalOptions())
      openSelect({ id: 'harness-remove-mode', title: `Remove ${plan.entry.label} · esc back`, value: selected, options: [
        { value: 'config', label: 'Remove configuration only', description: 'Keep installed files, history and credentials' },
        { value: 'cleanup', label: 'Remove configuration and private installation', disabled: !!plan.cleanupReason,
          description: plan.cleanupReason ?? plan.resources.join('\n') },
      ] }, { onCancel: back, onSubmit: mode => {
        const cleanup = mode === 'cleanup'
        if (mode !== 'config' && !cleanup) return
        openView({ id: 'harness-remove-confirm', title: `Remove ${plan.entry.label}? · enter remove · esc back`, nodes: [
          { id: 'target', kind: 'notice', level: 'warn', text: `Remove ${plan.entry.label} (${id}) and clear its saved default reference.` },
          { id: 'settings', kind: 'generic', title: 'Configuration file', body: settingsPath },
          { id: 'resources', kind: 'generic', title: cleanup ? 'Permanently delete private installation' : 'Keep installed files',
            body: cleanup ? plan.resources.join('\n') : 'No installed resources will be deleted.' },
          { id: 'preserved', kind: 'notice', level: 'info', text: 'History, credentials and shared caches are kept. Any active download for this Harness will be cancelled first. Esc returns to removal options without deleting anything.' },
        ] }, { onCancel: () => removal(id, back, mode), onSubmit: () => executeRemoval(plan, cleanup) })
      } })
    } catch (error) { retryView('Could not remove Harness', error, () => removal(id, back, selected)) }
  }

  async function executeRemoval(plan, cleanup) {
    const id = plan.entry.id
    if (removing.has(id)) return
    removing.add(id)
    try {
      // Recheck before cancellation, and again inside removeHarness after workers settle.
      planHarnessRemoval(settingsPath, id, removalOptions())
      configurationRevisions.set(id, (configurationRevisions.get(id) ?? 0) + 1)
      const jobs = [...downloads.values()].filter(job => job.entry.id === id)
      for (const job of jobs) job.controller.abort()
      openView({ id: 'harness-removing', title: `Removing ${plan.entry.label}`, nodes: [{
        id: 'working', kind: 'notice', level: 'info', text: 'Waiting for pending installation work to stop, then removing the confirmed configuration.',
      }] })
      await Promise.allSettled(jobs.map(job => job.task))
      if (disposed) return
      for (const job of jobs) downloads.delete(job.key)
      notifyDownloads()
      const result = removeHarness(settingsPath, plan, { ...removalOptions(), cleanup })
      candidateSnapshot = candidateSnapshot.filter(entry => entry.id !== id)
      pendingFailure = undefined
      refreshCommandChoices(); notifyDownloads()
      const active = ctx.tuiOverlay.active()
      if (active?.id === 'harness-removing') ownedOverlay?.close()
      const text = `${plan.entry.label} configuration removed. ` + (result.removed.length
        ? `Deleted private installation: ${result.removed.join(', ')}.` : 'Installed files were kept.')
        + ' History and credentials were kept.'
      // Do not replace a different panel opened while cancellation was settling.
      if (ctx.tuiOverlay.active() === null) openView({ id: 'harness-removed', title: 'Harness removed', nodes: [{
        id: 'removed', kind: 'notice', level: 'info', text,
      }] })
    } catch (error) {
      if (ctx.tuiOverlay.active()?.id === 'harness-removing') ownedOverlay?.close()
      retryView('Could not remove Harness', error, () => removal(id))
    } finally { removing.delete(id) }
  }

  async function save(id, preparedEntry, download, openNow = false) {
    const entry = preparedEntry ?? discoverHarnesses(settingsPath, options).find(entry => entry.id === id)
    if (entry === undefined) throw new Error(`unknown harness ${JSON.stringify(id)}`)
    if (removing.has(id)) throw new Error('This Harness is being removed; wait until removal finishes')
    upsertHarness(settingsPath, entry)
    setDefaultHarness(settingsPath, id)
    ctx.acpClient?.setDefaultAgent?.({ command: entry.command, args: entry.args ?? [],
      ...(entry.env !== undefined ? { env: entry.env } : {}) })
    refreshCommandChoices()
    if (download !== undefined && downloads.get(download.key) === download) {
      downloads.delete(download.key)
      notifyDownloads()
    }
    const newSession = () => ({ action: 'new-session' })
    if (openNow && !options.hostOwned) return newSession()
    openView({ id: 'harness-saved', title: 'Default Harness saved', nodes: [{
      id: 'notice', kind: 'notice', level: 'info',
      text: options.hostOwned
        ? `${entry.label} is saved for the next standalone session. The current profile owns its Harness.`
        : `${entry.label} will be used for new sessions. Enter opens a new tab now. Esc keeps the current session.`,
    }] }, options.hostOwned ? undefined : { onSubmit: newSession })
    if (!options.hostOwned) return { action: 'harness-selected', harness: {
      id: entry.id, label: entry.label, command: entry.command, args: entry.args ?? [],
      ...(entry.env !== undefined ? { env: entry.env } : {}),
    } }
  }

  function configured(entry) {
    if (removing.has(entry.id)) return
    configurationRevisions.set(entry.id, (configurationRevisions.get(entry.id) ?? 0) + 1)
    upsertHarness(settingsPath, entry)
    refreshCommandChoices()
    openView({ id: 'harness-saved', title: 'Harness configured', nodes: [{
      id: 'notice', kind: 'notice', level: 'info',
      text: `${entry.label} is configured. Your current Harness is unchanged. Use /harness when you want to switch.`,
    }] })
  }

  async function configure(id, tokens, scoped, query = '', displayedEntry) {
    if (disposed) return
    if (removing.has(id)) return
    const revision = configurationRevisions.get(id)
    try {
      if (!tokens.includes('--command')) {
        const candidate = displayedEntry ?? discoverHarnessCandidates(settingsPath, scoped).find((entry) => entry.id === id)
        if (candidate?.status?.startsWith('Available via ')) return install(candidate, scoped, query)
      }
      const entry = await addHarnessAsync(settingsPath, id, tokens, { ...scoped, persist: false })
      if (disposed || removing.has(id) || configurationRevisions.get(id) !== revision) return
      const runner = /^(npx|uvx)(?:\.(?:cmd|exe|bat))?$/i.exec(entry.command.split(/[\\/]/).at(-1))?.[1]?.toLowerCase()
      if (runner && entry.args?.[0] && !entry.args[0].startsWith('-')) {
        return install({ ...entry, distribution: { type: runner, command: entry.command, args: entry.args } }, scoped, query, entry)
      }
      return configured(entry)
    } catch (error) { retryView('Could not configure Harness', error, () => configure(id, tokens, scoped, query), query) }
  }

  function downloadOptions() {
    return [...downloads.values()].map((job) => ({
      value: `${DOWNLOAD}${job.key}`,
      label: `${job.state === 'ready' ? 'Downloaded' : job.state === 'failed' ? 'Download failed' : 'Downloading'} · ${job.entry.label}`,
      description: `${job.state === 'ready' ? 'View completed setup' : job.state === 'failed' ? 'View error and retry' : 'View download progress'} · ${commandText(job.entry)}`,
    }))
  }

  function matchingDownload(entry) {
    const identity = recipeIdentity(entry)
    return [...downloads.values()].find((job) => job.identity === identity)
  }

  function notifyDownloads() {
    if (disposed) return
    const jobs = [...downloads.values()].filter((job) => !job.acknowledged)
      .sort((left, right) => left.changed - right.changed)
    const latest = jobs.at(-1)
    downloadNotice?.update(latest === undefined ? [] : [{
      id: 'summary', kind: 'generic', body: '',
      title: `${latest.state === 'ready' ? 'Download complete' : latest.state === 'failed' ? 'Download failed' : 'Downloading'} · ${latest.entry.label}`
        + `${jobs.length > 1 ? ` · ${jobs.length} downloads` : ''} · /harness to view`,
      status: latest.state === 'running' ? 'running' : latest.state === 'ready' ? 'done' : 'err',
      tone: latest.state === 'failed' ? 'err' : latest.state === 'ready' ? 'ok' : 'brand',
    }])
  }

  function showDownload(job) {
    if (disposed) return
    visibleDownload = job
    const hide = () => { visibleDownload = undefined; notifyDownloads() }
    if (job.state === 'ready') {
      const acknowledge = () => {
        downloads.delete(job.key)
        hide()
      }
      openView({ id: 'harness-installing', title: `Download complete · ${job.entry.label} · enter new session · esc close`, nodes: [
        { id: 'complete', kind: 'notice', level: 'info', text: job.registered
          ? `${job.entry.label} is installed and configured.`
          : `${job.entry.label} is installed. Your newer configuration is unchanged.` },
        { id: 'next', kind: 'text', text: 'Setup is complete. Enter opens a new session with this Harness. Esc closes without switching.' },
      ] }, { onSubmit: () => {
        acknowledge()
        return save(job.entry.id, undefined, undefined, true)
      }, onCancel: acknowledge })
      return
    }
    if (job.state === 'failed') {
      openView({ id: 'harness-installing', title: `Download failed · ${job.entry.label} · enter retry · esc close`, nodes: [
        { id: 'error', kind: 'notice', level: 'error', text: errorText(job.error) },
        { id: 'next', kind: 'text', text: 'Enter retries the download. The current Harness and saved default are unchanged.' },
      ] }, { onSubmit: () => {
        hide()
        job.acknowledged = true
        notifyDownloads()
        downloads.delete(job.key)
        return install(job.entry, job.scoped, job.query, job.preparedEntry)
      }, onCancel: () => { job.acknowledged = true; hide() } })
      return
    }
    const { phase, receivedBytes, totalBytes, detail } = job.progress
    const label = ({ download: 'Downloading', extract: 'Extracting', verify: 'Verifying', complete: 'Finishing installation' })[phase] ?? 'Preparing'
    const size = receivedBytes === undefined ? '' : ` · ${(receivedBytes / 1048576).toFixed(1)} MB`
      + (totalBytes ? ` / ${(totalBytes / 1048576).toFixed(1)} MB` : '')
    openView({ id: 'harness-installing', title: `${label} ${job.entry.label} · esc run in background`, nodes: [
      { id: 'progress', kind: 'notice', level: 'info', text: `${label}${size}` },
      { id: 'location', kind: 'text', text: job.entry.installPath ?? `${job.entry.distribution.type} package cache` },
      ...(detail ? [{ id: 'detail', kind: 'text', text: detail }] : []),
      { id: 'hint', kind: 'text', text: 'Keep this panel open until the download finishes. Esc hides it; downloading continues while Martty is open.' },
    ] }, { onCancel: hide, onSubmit: () => showDownload(job) })
  }

  // The job belongs to the plugin, not its modal. Return immediately so a long
  // download cannot block the painter's ACP command queue or the old session.
  function install(entry, scoped, query, preparedEntry) {
    if (removing.has(entry.id)) return
    if (disposed) return
    const existing = matchingDownload(entry)
    if (existing !== undefined) return showDownload(existing)
    const key = downloads.has(entry.id) ? `${entry.id}:${++downloadSequence}` : entry.id
    const job = { key, identity: recipeIdentity(entry), changed: ++downloadRevision,
      entry, scoped, query, preparedEntry, controller: new AbortController(), state: 'running', progress: { phase: 'download' } }
    const revision = (configurationRevisions.get(entry.id) ?? 0) + 1
    configurationRevisions.set(entry.id, revision)
    downloads.set(key, job)
    const current = () => !disposed && !job.controller.signal.aborted && downloads.get(key) === job
    const ownsPanel = () => visibleDownload === job && ctx.tuiOverlay.active()?.id === 'harness-installing'
    const progress = (snapshot) => {
      if (!current()) return
      const now = Date.now()
      const redraw = snapshot.phase !== job.progress.phase || snapshot.detail !== job.progress.detail || now - (job.lastUpdate ?? 0) >= 100
      job.progress = snapshot
      if (redraw && ownsPanel()) { job.lastUpdate = now; showDownload(job) }
    }
    const finish = () => {
      if (!current()) return
      const foreground = ownsPanel()
      job.changed = ++downloadRevision
      notifyDownloads()
      if (foreground) showDownload(job)
    }
    showDownload(job)
    notifyDownloads()
    job.task = (async () => {
      const preparation = { ...scoped, settingsPath, persist: false, signal: job.controller.signal, onProgress: progress }
      if (entry.distribution.type === 'npx' || entry.distribution.type === 'uvx') {
        const prepare = options.preparePackage ?? (await import('./harness-package.js')).prepareHarnessPackage
        await prepare(entry, preparation)
      }
      if (!current()) return
      job.configured = preparedEntry ?? await addHarnessAsync(settingsPath, entry.id, [], preparation)
      if (!current()) return
      if (configurationRevisions.get(entry.id) === revision) {
        upsertHarness(settingsPath, job.configured)
        refreshCommandChoices()
        job.registered = true
      }
      job.state = 'ready'
      finish()
    })().catch((error) => {
      if (!current()) return
      job.state = 'failed'
      job.error = error
      finish()
    })
  }

  function choose(entry, scoped, query) {
    if (removing.has(entry.id)) return
    if (entry.status === 'Configured') return save(entry.id)
    const downloading = matchingDownload(entry)
    if (downloading !== undefined) return showDownload(downloading)
    if (entry.status === 'Available to install') {
      openSelect({ id: 'harness-install-confirm', title: `Install ${entry.label} in Martty?`, value: 'install', options: [
        { value: 'install', label: `Install ${entry.label}`, description: `download to ${entry.installPath ?? "Martty's private bin directory"}` },
        { value: 'cancel', label: 'Back', description: 'Choose another Harness' },
      ] }, { onSubmit: (action) => action === 'install' ? install(entry, scoped, query) : browse(query) })
      return
    }
    if (entry.status === 'Needs npx' || entry.status === 'Needs uvx') {
      const runner = entry.distribution.type
      const runtime = runner === 'npx' ? 'Node.js/npm' : 'uv'
      const url = runner === 'npx' ? 'https://nodejs.org/en/download' : 'https://docs.astral.sh/uv/getting-started/installation/'
      openSelect({ id: 'harness-runner-missing', title: `${entry.label} needs ${runtime}`, value: ':recheck', options: [
        { value: ':recheck', label: 'Recheck installation', description: `Install ${runtime}: ${url}` },
        { value: MANUAL, label: 'Use an existing ACP command…', description: 'The program may be installed outside PATH' },
        { value: BACK, label: 'Back to Harnesses', description: 'Choose another distribution' },
      ] }, { onSubmit: (action) => action === MANUAL ? manual(entry.id) : browse(query, action === ':recheck') })
      return
    }
    if (entry.status?.startsWith('Available via ')) {
      openSelect({ id: 'harness-package-confirm', title: `Download ${entry.label}?`, value: 'configure', options: [
        { value: 'configure', label: 'Download', description: `Prepare packages with ${entry.distribution.type}, then connect when ready` },
        { value: BACK, label: 'Back', description: 'Choose another Harness' },
      ] }, { onSubmit: (action) => action === 'configure' ? install(entry, scoped, query) : browse(query) })
      return
    }
    if (entry.status === 'Not installed') {
      const instruction = entry.fallback === undefined
        ? `Install: \`${commandText(entry.install ?? {})}\`\nThen return here and press Enter to check again.`
        : `Configure with /harness add ${entry.id}; this saves the registry fallback (\`${commandText(entry.install ?? {})}\`) as the launch command.`
      openView({ id: 'harness-find-install', title: `${entry.label} is not installed`, nodes: [{
        id: 'instructions', kind: 'markdown', text: instruction,
      }] }, { onSubmit: () => browse(query, true) })
      return
    }
    // A local path is the recipe the user selected. Do not silently turn a
    // vanished executable into an unconfirmed download on a second scan.
    return configured({ ...entry, command: entry.resolvedCommand ?? entry.command })
  }

  function browse(query = '', refresh = false) {
    if (disposed) return
    operation?.abort()
    const version = ++flow
    const controller = new AbortController()
    operation = controller
    let registryLoaded = false
    const cancel = () => {
      ++flow; controller.abort(); operation = undefined
      if (!registryLoaded) registryPromise = undefined
    }
    const current = () => !disposed && version === flow
    const scan = options.scanCandidates ?? scanHarnessCandidates
    const scanOptions = { ...options, settingsPath, signal: controller.signal }
    let complete = false
    let registryFailure
    function show(candidates, registry, pending = false, failure) {
      if (!current()) return
      const entries = [...candidates].sort((left, right) => currentFirst(left, right)
        || Number(locallyAvailable(right)) - Number(locallyAvailable(left)))
      const scoped = { ...options, registry }
      openSelect({ id: 'harness-find', title: failure ? 'Add Harness · Registry offline' : pending ? 'Add Harness · checking Registry…' : 'Add Harness', searchable: true,
        value: entries[0]?.id ?? REFRESH,
        options: [
          ...entries.map((entry) => ({ value: entry.id, label: entry.label, description: findDescription(entry),
            ...currentOption(entry),
            group: locallyAvailable(entry) ? 'Installed / configured' : entry.status === 'Checking locally' ? 'Catalog' : 'Not downloaded' })),
          { value: REFRESH, label: failure ? 'Retry Registry' : 'Refresh Registry', description: failure ? errorText(failure) : 'Recheck the catalog and local programs' },
          manualOption,
        ],
      }, { onCancel: cancel, onSubmit: (id) => {
        cancel()
        if (id === REFRESH) return browse(query, true)
        if (id === MANUAL) return manual()
        // Selection acts on the rendered snapshot. Re-discovery here used to scan
        // every local environment again before even showing the next panel.
        const entry = entries.find((candidate) => candidate.id === id)
        if (entry !== undefined) {
          const selectedOptions = { ...scoped, registry: registry.filter((record) => record.id === id) }
          if (entry.status !== 'Checking locally') return choose(entry, selectedOptions, query)
          const selectionVersion = flow
          return scan(settingsPath, selectedOptions, query).then((entries) => {
            if (disposed || flow !== selectionVersion) return
            const resolved = entries.find((candidate) => candidate.id === id)
            if (resolved !== undefined) return choose(resolved, selectedOptions, query)
          }).catch((error) => retryView('Could not check Harness', error, () => browse(query)))
        }
      } })
    }
    // The local worker and network request start independently. A slow catalog
    // never prevents choosing a saved/local command or leaving the panel.
    const saved = discoverHarnesses(settingsPath, { ...options, registry: [], pathValue: '' })
      .map((entry) => ({ ...entry, status: 'Configured' }))
    const known = new Map([...candidateSnapshot, ...saved].map((entry) => [entry.id, entry]))
    const preview = [...known.values(), ...registrySnapshot.filter((record) => !known.has(record.id))
      .map((record) => ({ id: record.id, label: record.label, status: 'Checking locally', source: 'registry' }))]
      .filter((entry) => !query || `${entry.id} ${entry.label}`.toLowerCase().includes(query.toLowerCase()))
    let displayed = preview
    show(displayed, registrySnapshot, true)
    const local = scan(settingsPath, { ...scanOptions, registry: registrySnapshot }, query)
    void local.then((entries) => {
      if ((!complete || registryFailure !== undefined) && current()) {
        candidateSnapshot = entries
        displayed = entries
        show(entries, registrySnapshot, registryFailure === undefined, registryFailure)
      }
    }).catch(() => {})
    void (async () => { try {
      const registry = await loadRegistry(refresh, controller.signal)
      registryLoaded = true
      if (!current()) return
      const entries = await scan(settingsPath, { ...scanOptions, registry }, query)
      if (!current()) return
      complete = true
      candidateSnapshot = entries
      show(entries, registry)
    } catch (error) {
      if (!current()) return
      registryFailure = error
      complete = true
      show(displayed, registrySnapshot, false, error)
    } })()
  }

  commandRegistration = ctx.tuiCommands.register({ name: 'harness', description: 'Choose the default Harness for new sessions',
    input: { hint: '[id] [--new] | add | remove [id] | find [query]', options: commandChoices() },
  }, async (args) => {
    if (disposed) return
    refreshCommandChoices()
    if (args.trim() === '' && pendingFailure !== undefined) {
      const { title, error, retry, query } = pendingFailure
      pendingFailure = undefined
      return retryView(title, error, retry, query)
    }
    let tokens
    try { tokens = tokenizeHarnessArgs(args.trim()) } catch (error) {
      retryView('Could not read Harness command', error, () => manual())
      return
    }
    if (tokens[0] === 'remove') return removal(tokens[1])
    if (tokens[0] === 'find' || (tokens[0] === 'add' && tokens[1] === undefined)) return browse(tokens.slice(1).join(' '))
    if (tokens[0] === 'add') {
      try {
        if (tokens.slice(2).includes('--command')) return configure(tokens[1], tokens.slice(2), options)
        const scoped = { ...options, registry: (await loadRegistry()).filter((record) => record.id === tokens[1]) }
        const entry = discoverHarnessCandidates(settingsPath, scoped).find((candidate) => candidate.id === tokens[1])
        if (!tokens.slice(2).includes('--command') && (entry?.status === 'Available to install' || entry?.status?.startsWith('Available via '))) return choose(entry, scoped, '')
        return configure(tokens[1], tokens.slice(2), scoped)
      } catch (error) { retryView('Could not configure Harness', error, () => browse()) }
      return
    }
    if (tokens[0] !== undefined) {
      const saved = discoverHarnesses(settingsPath, options).find((entry) => entry.id === tokens[0])
      if (saved !== undefined) return save(saved.id, saved, undefined, tokens.includes('--new'))
      const job = [...downloads.values()].filter((job) => job.entry.id === tokens[0]).at(-1)
      return job === undefined ? save(tokens[0]) : showDownload(job)
    }
    showHarnessPicker()
  })

  function showHarnessPicker(selected) {
    const entries = choices()
    const saved = savedHarnesses(settingsPath)
    const forced = [options.forcedHarness, ...(options.defaults ?? []).filter(entry => entry.source === 'forced')].filter(Boolean)
    // Eligibility does not walk installation trees; inspect resource ownership only on Delete.
    for (const option of entries) {
      const entry = saved.find(entry => entry.id === option.value)
      if (entry && !isRunning(entry) && !forced.some(value => value.id === entry.id || value.command === entry.command)) {
        option.deletable = true
      }
    }
    const downloaded = downloadOptions()
    if (entries.length === 0 && downloaded.length === 0) return browse()
    openSelect({ id: 'harness', title: 'Default Harness · for new sessions',
      value: (entries.find(entry => entry.value === selected) ?? entries.find((entry) => entry.value === selectedHarness(settingsPath)?.id) ?? entries[0] ?? downloaded[0]).value,
      options: [...entries, ...downloaded, addOption],
    }, { onDelete: id => removal(id),
      onSubmit: (id) => id === ADD ? browse() : id.startsWith(DOWNLOAD) ? showDownload(downloads.get(id.slice(DOWNLOAD.length))) : save(id) })
  }
  return () => {
    disposed = true; ++flow; operation?.abort()
    for (const job of downloads.values()) job.controller.abort()
    ownedOverlay?.close(); stopDownloadSlot?.(); commandRegistration?.()
  }
}
