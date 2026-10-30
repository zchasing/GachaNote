<script setup lang="ts">
import { open } from '@tauri-apps/plugin-dialog'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import type {
  CredentialDto,
  DiscoveredUrl,
  FetchModeDto,
  FilePreviewDto,
  HgRoleDto,
  PreviewAccountDto,
  TokenSource,
} from '@/bindings'
import { api, inTauri } from '@/api'
import { fmtDate, fmtInt } from '@/composables/format'
import { useErrors } from '@/composables/useErrors'
import { useNames } from '@/composables/useNames'
import { useAccountsStore } from '@/stores/accounts'
import { useGamesStore } from '@/stores/games'
import { useTasksStore } from '@/stores/tasks'
import type { ImportRequest } from '@/stores/ui'
import AppIcon from './AppIcon.vue'
import GameIcon from './GameIcon.vue'
import SegmentedControl from './SegmentedControl.vue'

// 导入 / 拉取向导（UI/UX 文档 4.1、4.4）：选择游戏 → 选择方式 → 准备 → 进度 → 报告。
const props = defineProps<{ request: ImportRequest }>()
const emit = defineEmits<{ close: [] }>()
const router = useRouter()
const games = useGamesStore()
const accounts = useAccountsStore()
const tasks = useTasksStore()
const { describe } = useErrors()
const { t, locale, gameName } = useNames()

type Step = 'game' | 'method' | 'prepare' | 'progress' | 'report'
/** `login` / `token` / `saved` 为鹰角 token 拉取：登录窗口、手动粘贴、已保存的 token。 */
type Method = 'url' | 'file' | 'login' | 'token' | 'saved'
const STEPS: Step[] = ['game', 'method', 'prepare', 'progress', 'report']

/** 选择的游戏；`null` 表示直接导入文件、自动识别游戏。 */
const game = ref<string | null>(props.request.game ?? null)
const method = ref<Method>(props.request.method ?? 'url')
const step = ref<Step>(initialStep())

function initialStep(): Step {
  const { game: g, method: m } = props.request
  if (g) return m ? 'prepare' : 'method'
  if (m === 'file') return 'prepare'
  return 'game'
}

const info = computed(() => (game.value ? games.game(game.value) : undefined))
const startError = ref<{ title: string; hint: string; detail: string | null } | null>(null)

function pickGame(id: string | null) {
  game.value = id
  startError.value = null
  if (id === null) {
    method.value = 'file'
    step.value = 'prepare'
    return
  }
  // 从总览的「从游戏拉取」进来时，游戏支持就直接进入准备步骤
  if (props.request.method === 'url' && games.game(id)?.canFetch && games.game(id)?.fetch === 'url') {
    method.value = 'url'
    step.value = 'prepare'
  } else {
    step.value = 'method'
  }
}

function pickMethod(m: Method) {
  method.value = m
  startError.value = null
  resetToken()
  step.value = 'prepare'
  if (m === 'saved' && savedOfGame.value[0]) void useSaved(savedOfGame.value[0])
}

function back() {
  startError.value = null
  if (step.value === 'prepare') step.value = game.value ? 'method' : 'game'
  else if (step.value === 'method') step.value = 'game'
}

const formatList = computed(() =>
  (info.value?.formats ?? []).map((f) => t(`import.formatNames.${f}`)).join('、'),
)

// ── 链接拉取 ──────────────────────────────────────────────
const canDiscover = ref(false)
const discovering = ref(false)
const discovered = ref<DiscoveredUrl | null>(null)
const discoverError = ref<string | null>(null)
const pasted = ref('')
const useSource = ref<'discovered' | 'pasted'>('discovered')
const mode = ref<FetchModeDto>('incremental')
const hasAccount = computed(() => (game.value ? accounts.ofGame(game.value).length > 0 : false))

async function discover() {
  if (!game.value) return
  discovering.value = true
  discoverError.value = null
  discovered.value = null
  try {
    discovered.value = await api.discoverUrl(game.value)
    useSource.value = 'discovered'
  } catch (e) {
    discoverError.value = describe(e).error.code
    useSource.value = 'pasted'
  } finally {
    discovering.value = false
  }
}

watch(
  [step, method, game],
  async () => {
    if (step.value !== 'prepare' || method.value !== 'url' || !game.value) return
    if (inTauri()) {
      try {
        canDiscover.value = (await api.appInfo()).canDiscover
      } catch {
        canDiscover.value = false
      }
    }
    // 没有账号时只能全部拉取
    mode.value = hasAccount.value ? 'incremental' : 'full'
    if (canDiscover.value) void discover()
    else useSource.value = 'pasted'
  },
  { immediate: true },
)

const canStartUrl = computed(() =>
  useSource.value === 'discovered' ? !!discovered.value : pasted.value.trim().startsWith('http'),
)

async function startUrl() {
  if (!game.value) return
  startError.value = null
  try {
    const source =
      useSource.value === 'discovered' && discovered.value
        ? ({ kind: 'discovered', id: discovered.value.sourceId } as const)
        : ({ kind: 'pasted', url: pasted.value.trim() } as const)
    taskId.value = await api.fetchByUrl(game.value, source, mode.value)
    step.value = 'progress'
  } catch (e) {
    showStartError(e)
  }
}

// ── 鹰角 token ───────────────────────────────────────────
const canRemember = ref(false)
const credentials = ref<CredentialDto[]>([])
const savedOfGame = computed(() => credentials.value.filter((c) => c.game === game.value))
const tokenSource = ref<TokenSource | null>(null)
const roles = ref<HgRoleDto[] | null>(null)
const chosenRoles = ref<Set<string>>(new Set())
const remember = ref(true)
const loggingIn = ref(false)
const loadingRoles = ref(false)
const loginError = ref<string | null>(null)
const pastedToken = ref('')
const savedAccount = ref<string | null>(null)

watch(
  [step, game],
  async () => {
    if (step.value !== 'method' || info.value?.vendor !== 'hypergryph' || !inTauri()) return
    try {
      const [app, creds] = await Promise.all([api.appInfo(), api.listCredentials()])
      canRemember.value = app.canRememberToken
      credentials.value = creds
    } catch {
      credentials.value = []
    }
  },
  { immediate: true },
)

function resetToken() {
  tokenSource.value = null
  roles.value = null
  loginError.value = null
  savedAccount.value = null
  mode.value = hasAccount.value ? 'incremental' : 'full'
  remember.value = canRemember.value
}

async function loadRoles(source: TokenSource) {
  loadingRoles.value = true
  startError.value = null
  roles.value = null
  try {
    const list = await api.hgRoles(source)
    tokenSource.value = source
    roles.value = list
    // 默认全选
    chosenRoles.value = new Set(list.map(key))
  } catch (e) {
    showStartError(e)
  } finally {
    loadingRoles.value = false
  }
}

async function login() {
  loggingIn.value = true
  loginError.value = null
  startError.value = null
  try {
    const id = await api.hgLogin(t('import.hg.windowTitle'), t('import.hg.loginHint'))
    await loadRoles({ kind: 'login', id })
  } catch (e) {
    const code = describe(e).error.code
    loginError.value = code === 'cancelled' ? 'closed' : code === 'network' ? 'timeout' : null
    if (!loginError.value) showStartError(e)
  } finally {
    loggingIn.value = false
  }
}

function useSaved(c: CredentialDto) {
  savedAccount.value = c.accountId
  remember.value = false
  return loadRoles({ kind: 'saved', accountId: c.accountId })
}

function switchToPaste() {
  method.value = 'token'
  resetToken()
}

function toggleRole(r: HgRoleDto) {
  const next = new Set(chosenRoles.value)
  if (next.has(key(r))) next.delete(key(r))
  else next.add(key(r))
  chosenRoles.value = next
}

function openPage(page: 'hgLogin' | 'hgToken') {
  api.openExternal(page).catch(showStartError)
}

async function startToken() {
  if (!tokenSource.value || !roles.value) return
  startError.value = null
  const picked = roles.value.filter((r) => chosenRoles.value.has(key(r))).map((r) => ({ game: r.game, uid: r.uid }))
  try {
    taskId.value = await api.fetchByToken(tokenSource.value, picked, remember.value, mode.value)
    step.value = 'progress'
  } catch (e) {
    showStartError(e)
  }
}

const isToken = computed(() => method.value === 'login' || method.value === 'token' || method.value === 'saved')

// ── 文件导入 ──────────────────────────────────────────────
const sniffing = ref(false)
const preview = ref<FilePreviewDto | null>(null)
const selected = ref<Set<string>>(new Set())
const dragging = ref(false)
const key = (a: { game: string; uid: string }) => `${a.game}:${a.uid}`

async function chooseFile() {
  const path = await open({
    multiple: false,
    directory: false,
    filters: [{ name: t('import.file.filterName'), extensions: ['json', 'csv'] }],
  })
  if (typeof path === 'string') await sniff(path)
}

async function sniff(path: string) {
  sniffing.value = true
  startError.value = null
  preview.value = null
  try {
    const p = await api.sniffFile(path)
    preview.value = p
    // 默认选中：所选游戏的账号；自动识别时选中全部支持的账号
    selected.value = new Set(
      p.accounts.filter((a) => a.supported && (!game.value || a.game === game.value)).map(key),
    )
  } catch (e) {
    showStartError(e)
  } finally {
    sniffing.value = false
  }
}

function toggle(a: PreviewAccountDto) {
  const next = new Set(selected.value)
  if (next.has(key(a))) next.delete(key(a))
  else next.add(key(a))
  selected.value = next
}

const noneOfGame = computed(
  () => !!preview.value && !!game.value && !preview.value.accounts.some((a) => a.game === game.value),
)

async function startFile() {
  if (!preview.value) return
  startError.value = null
  const only = preview.value.accounts
    .filter((a) => selected.value.has(key(a)))
    .map((a) => ({ game: a.game, uid: a.uid }))
  try {
    taskId.value = await api.importFile(preview.value.path, only)
    step.value = 'progress'
  } catch (e) {
    showStartError(e)
  }
}

// 拖放文件：只有 Tauri 窗口能拿到文件路径
let unlistenDrop: (() => void) | null = null
onMounted(async () => {
  if (!inTauri()) return
  try {
    unlistenDrop = await listenDrop()
  } catch {
    // 拿不到当前窗口（例如测试环境）时只保留「选择文件」
  }
})
onBeforeUnmount(() => unlistenDrop?.())

function listenDrop() {
  return getCurrentWebview().onDragDropEvent((e) => {
    if (step.value !== 'prepare' || method.value !== 'file') return
    if (e.payload.type === 'over' || e.payload.type === 'enter') dragging.value = true
    else if (e.payload.type === 'leave') dragging.value = false
    else if (e.payload.type === 'drop') {
      dragging.value = false
      const path = e.payload.paths[0]
      if (path) void sniff(path)
    }
  })
}

// ── 进度与报告 ────────────────────────────────────────────
const taskId = ref<number | null>(null)
const task = computed(() => (taskId.value === null ? null : (tasks.tasks.get(taskId.value) ?? null)))
const cancelling = ref(false)

watch(
  () => task.value?.status,
  (s) => {
    if (s === 'done' || s === 'failed') step.value = 'report'
  },
)

async function cancel() {
  if (taskId.value === null) return
  cancelling.value = true
  await api.cancelTask(taskId.value).catch(() => false)
}

const stageText = computed(() => {
  const tk = task.value
  if (!tk?.stage) return t('import.progress.starting')
  const base = t(`import.progress.${tk.stage}`)
  if (tk.stage === 'fetch') return `${base} · ${t('import.progress.fetched', { n: fmtInt(tk.current, locale.value) })}`
  if (tk.stage === 'write' && tk.total)
    return `${base} · ${t('import.progress.written', {
      current: fmtInt(tk.current, locale.value),
      total: fmtInt(tk.total, locale.value),
    })}`
  return base
})
const progressPct = computed(() => {
  const tk = task.value
  if (!tk?.total) return null
  return `${Math.min(100, (tk.current / tk.total) * 100)}%`
})

const report = computed(() => task.value?.report ?? null)
const failure = computed(() => {
  const err = task.value?.error
  if (!err) return null
  return { ...describe(err, isToken.value ? 'token' : undefined), cancelled: err.code === 'cancelled' }
})
const created = computed(() =>
  (report.value?.accounts ?? []).filter((a) => a.created).map((a) => `${gameName(a.game)} ${a.uid}`),
)
const FAILURES_SHOWN = 20

function viewAccount() {
  const first = report.value?.accounts[0]
  emit('close')
  if (first) void router.push(`/account/${first.accountId}/stats`)
}

function showStartError(e: unknown) {
  const d = describe(e, isToken.value ? 'token' : undefined)
  startError.value = { title: d.title, hint: d.hint, detail: d.error.detail ?? d.error.message }
}

const stepIndex = computed(() => STEPS.indexOf(step.value))
const running = computed(() => step.value === 'progress')
</script>

<template>
  <div class="overlay" @click.self="!running && emit('close')">
    <div
      class="modal wide"
      role="dialog"
      aria-modal="true"
      :aria-label="t('import.title')"
      data-test="import-wizard"
      @keydown.escape="!running && emit('close')"
    >
      <div class="row" style="justify-content: space-between">
        <h2 class="ct">{{ t('import.title') }}</h2>
        <button class="iconbtn" :aria-label="t('common.close')" @click="emit('close')">
          <AppIcon name="close" />
        </button>
      </div>
      <ol class="steps">
        <li v-for="(s, i) in STEPS" :key="s" :class="{ on: i === stepIndex, done: i < stepIndex }">
          <span class="sn">{{ i + 1 }}</span>{{ t(`import.steps.${s}`) }}
        </li>
      </ol>

      <!-- 1. 选择游戏 -->
      <template v-if="step === 'game'">
        <div class="muted">{{ t('import.pickGame') }}</div>
        <div class="ggrid">
          <button
            v-for="g in games.games"
            :key="g.id"
            class="gcard"
            :data-test="`pick-${g.id}`"
            @click="pickGame(g.id)"
          >
            <GameIcon :game="g.id" :size="32" />
            <span>{{ gameName(g.id) }}</span>
          </button>
        </div>
        <button class="gcard auto" data-test="pick-auto" @click="pickGame(null)">
          <AppIcon name="file" :size="20" />
          <span style="text-align: left">
            {{ t('import.autoDetect') }}
            <span class="muted" style="display: block">{{ t('import.autoDetectHint') }}</span>
          </span>
        </button>
      </template>

      <!-- 2. 选择方式 -->
      <template v-else-if="step === 'method' && info">
        <div class="row">
          <GameIcon :game="info.id" :size="24" /><b>{{ gameName(info.id) }}</b>
          <button class="linkbtn" @click="step = 'game'">{{ t('import.changeGame') }}</button>
        </div>
        <div class="mgrid">
          <button
            v-if="info.canFetch && info.fetch === 'url'"
            class="mcard"
            data-test="method-url"
            @click="pickMethod('url')"
          >
            <AppIcon name="link" :size="20" />
            <b>{{ t('import.methods.url') }}</b>
            <span class="muted">{{ info.id === 'wuwa' ? t('import.methods.urlDescWuwa') : t('import.methods.urlDesc') }}</span>
          </button>
          <template v-if="info.vendor === 'hypergryph' && info.canFetch">
            <button v-if="savedOfGame.length" class="mcard" data-test="method-saved" @click="pickMethod('saved')">
              <AppIcon name="refresh" :size="20" />
              <b>{{ t('import.methods.saved') }}</b>
              <span class="muted">{{ t('import.methods.savedDesc') }}</span>
            </button>
            <button class="mcard" data-test="method-login" @click="pickMethod('login')">
              <AppIcon name="user" :size="20" />
              <b>{{ t('import.methods.login') }} <span class="ver">{{ t('import.methods.recommended') }}</span></b>
              <span class="muted">{{ t('import.methods.loginDesc') }}</span>
            </button>
            <button class="mcard" data-test="method-token" @click="pickMethod('token')">
              <AppIcon name="key" :size="20" />
              <b>{{ t('import.methods.token') }}</b>
              <span class="muted">{{ t('import.methods.tokenDesc') }}</span>
            </button>
          </template>
          <button class="mcard" data-test="method-file" @click="pickMethod('file')">
            <AppIcon name="file" :size="20" />
            <b>{{ t('import.methods.file') }}</b>
            <span class="muted">{{ t('import.methods.fileDesc', { formats: formatList }) }}</span>
          </button>
        </div>
      </template>

      <!-- 3a. 准备：链接 -->
      <template v-else-if="step === 'prepare' && method === 'url' && info">
        <div v-if="canDiscover" class="card" style="gap: 8px">
          <div v-if="discovering" class="muted">{{ t('import.url.searching') }}</div>
          <template v-else-if="discovered">
            <label class="check">
              <input v-model="useSource" type="radio" value="discovered" />
              {{ t('import.url.found', { display: discovered.display }) }}
            </label>
          </template>
          <div v-else-if="discoverError" class="notice error">{{ t('import.url.notFound') }}</div>
          <button class="linkbtn" style="align-self: flex-start" :disabled="discovering" @click="discover">
            {{ t('import.url.retry') }}
          </button>
        </div>
        <div v-else class="notice info">{{ t('import.url.unsupported') }}</div>
        <label class="field">
          <span class="row" style="gap: 6px">
            <input v-if="canDiscover && discovered" v-model="useSource" type="radio" value="pasted" />
            {{ t('import.url.paste') }}
          </span>
          <textarea
            v-model="pasted"
            class="input"
            :placeholder="t('import.url.pastePlaceholder')"
            data-test="paste-url"
            @focus="useSource = 'pasted'"
          />
        </label>
        <div class="field">
          {{ t('import.url.mode') }}
          <SegmentedControl
            v-model="mode"
            :options="[
              { value: 'incremental', label: t('import.url.incremental') },
              { value: 'full', label: t('import.url.full') },
            ]"
          />
        </div>
      </template>

      <!-- 3c. 准备：鹰角 token -->
      <template v-else-if="step === 'prepare' && isToken && info">
        <template v-if="!roles">
          <template v-if="method === 'login'">
            <div class="notice info">{{ t('import.hg.loginHint') }}</div>
            <div v-if="loginError" class="notice error" data-test="login-error">
              <span>{{ t(`import.hg.login_${loginError}`) }}</span>
              <button class="linkbtn" style="align-self: flex-start" @click="switchToPaste">
                {{ t('import.hg.usePaste') }}
              </button>
            </div>
            <button class="btn primary" style="align-self: flex-start" :disabled="loggingIn || loadingRoles" data-test="hg-login" @click="login">
              <AppIcon name="user" />{{ loggingIn ? t('import.hg.waiting') : t('import.hg.openLogin') }}
            </button>
          </template>
          <template v-else-if="method === 'token'">
            <ol class="guide">
              <li>
                {{ t('import.hg.step1') }}
                <button class="linkbtn" @click="openPage('hgLogin')">{{ t('import.hg.openSite') }}</button>
              </li>
              <li>
                {{ t('import.hg.step2') }}
                <button class="linkbtn" @click="openPage('hgToken')">{{ t('import.hg.openToken') }}</button>
              </li>
              <li>{{ t('import.hg.step3') }}</li>
            </ol>
            <textarea
              v-model="pastedToken"
              class="input"
              :placeholder="t('import.hg.pastePlaceholder')"
              data-test="paste-token"
            />
            <div class="notice info">{{ t('import.hg.tokenSafety') }}</div>
          </template>
          <template v-else-if="method === 'saved'">
            <div class="field">
              {{ t('import.hg.pickSaved') }}
              <label v-for="c in savedOfGame" :key="c.accountId" class="check">
                <input type="radio" :checked="savedAccount === c.accountId" @change="useSaved(c)" />
                {{ c.nickname ?? c.uid }} <span class="muted">{{ t('account.uid', { uid: c.uid }) }}</span>
              </label>
            </div>
          </template>
          <div v-if="loadingRoles" class="muted">{{ t('import.hg.loadingRoles') }}</div>
        </template>
        <template v-else>
          <div v-if="roles.length === 0" class="notice error">{{ t('import.hg.noRoles') }}</div>
          <div v-else class="tablebox">
            <table data-test="role-table">
              <thead>
                <tr>
                  <th style="width: 32px" />
                  <th>{{ t('import.file.colGame') }}</th>
                  <th>{{ t('import.hg.colNickname') }}</th>
                  <th>{{ t('import.file.colUid') }}</th>
                  <th>{{ t('import.hg.colChannel') }}</th>
                  <th>{{ t('import.file.colStatus') }}</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="r in roles" :key="key(r)">
                  <td>
                    <input
                      type="checkbox"
                      :checked="chosenRoles.has(key(r))"
                      :aria-label="`${gameName(r.game)} ${r.uid}`"
                      @change="toggleRole(r)"
                    />
                  </td>
                  <td>{{ gameName(r.game) }}</td>
                  <td>{{ r.nickname ?? '—' }}</td>
                  <td class="num">{{ r.uid }}</td>
                  <td>{{ r.channel ?? '—' }}</td>
                  <td>
                    <span v-if="r.exists" class="tag neutral">{{ t('import.file.exists') }}</span>
                    <span v-else class="tag up">{{ t('import.file.new') }}</span>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
          <label v-if="canRemember && method !== 'saved'" class="check">
            <input v-model="remember" type="checkbox" data-test="remember-token" />
            {{ t('import.hg.remember') }}
          </label>
          <div class="field">
            {{ t('import.url.mode') }}
            <SegmentedControl
              v-model="mode"
              :options="[
                { value: 'incremental', label: t('import.url.incremental') },
                { value: 'full', label: t('import.url.full') },
              ]"
            />
          </div>
        </template>
      </template>

      <!-- 3b. 准备：文件 -->
      <template v-else-if="step === 'prepare' && method === 'file'">
        <div v-if="!preview" class="drop" :class="{ over: dragging }" data-test="drop-zone">
          <AppIcon name="file" :size="28" />
          <div v-if="sniffing">{{ t('import.file.reading') }}</div>
          <div v-else>
            {{ t('import.file.drop') }}
            <button class="linkbtn" data-test="choose-file" @click="chooseFile">{{ t('import.file.choose') }}</button>
          </div>
        </div>
        <template v-else>
          <div class="row">
            <span>{{ t('import.file.detected', { file: preview.fileName, format: t(`import.formatNames.${preview.format}`) }) }}</span>
            <button class="linkbtn" @click="chooseFile">{{ t('import.file.choose') }}</button>
          </div>
          <div v-if="noneOfGame && game" class="notice info">
            {{ t('import.file.otherGame', { game: gameName(game) }) }}
          </div>
          <div class="tablebox">
            <table data-test="preview-table">
              <thead>
                <tr>
                  <th style="width: 32px" />
                  <th>{{ t('import.file.colGame') }}</th>
                  <th>{{ t('import.file.colUid') }}</th>
                  <th style="text-align: right">{{ t('import.file.colCount') }}</th>
                  <th>{{ t('import.file.colRange') }}</th>
                  <th>{{ t('import.file.colStatus') }}</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="a in preview.accounts" :key="key(a)">
                  <td>
                    <input
                      type="checkbox"
                      :checked="selected.has(key(a))"
                      :disabled="!a.supported"
                      :aria-label="`${gameName(a.game)} ${a.uid}`"
                      @change="toggle(a)"
                    />
                  </td>
                  <td>{{ gameName(a.game) }}</td>
                  <td class="num">{{ a.uid }}</td>
                  <td class="num" style="text-align: right">{{ fmtInt(a.count, locale) }}</td>
                  <td class="num muted">
                    <template v-if="a.from && a.to">{{ fmtDate(a.from) }} – {{ fmtDate(a.to) }}</template>
                  </td>
                  <td>
                    <span v-if="!a.supported" class="tag neutral">{{ t('import.file.unsupported') }}</span>
                    <span v-else-if="a.exists" class="tag neutral">{{ t('import.file.exists') }}</span>
                    <span v-else class="tag up">{{ t('import.file.new') }}</span>
                    <span v-if="a.failed" class="tag loss" style="margin-left: 4px">
                      {{ t('import.file.failed', { n: a.failed }) }}
                    </span>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </template>
      </template>

      <!-- 4. 进度 -->
      <template v-else-if="step === 'progress'">
        <div class="card" style="gap: 10px" data-test="progress">
          <div>{{ cancelling ? t('import.progress.cancelling') : stageText }}</div>
          <div class="track">
            <div v-if="progressPct" class="fill" :style="{ width: progressPct }" />
            <div v-else class="fill indeterminate" />
          </div>
        </div>
      </template>

      <!-- 5. 报告 -->
      <template v-else-if="step === 'report'">
        <template v-if="report">
          <div class="rgrid" data-test="report">
            <div class="card"><span class="muted">{{ t('import.report.added') }}</span><b class="num big">{{ fmtInt(report.added, locale) }}</b></div>
            <div class="card"><span class="muted">{{ t('import.report.skipped') }}</span><b class="num big">{{ fmtInt(report.skipped, locale) }}</b></div>
            <div class="card"><span class="muted">{{ t('import.report.failed') }}</span><b class="num big" :class="{ bad: report.failed }">{{ fmtInt(report.failed, locale) }}</b></div>
          </div>
          <div v-if="report.accounts.length === 0" class="muted">{{ t('import.report.nothing') }}</div>
          <div v-if="created.length" class="muted">{{ t('import.report.created', { list: created.join('、') }) }}</div>
          <div v-if="report.backup" class="muted">{{ t('import.report.backup', { name: report.backup }) }}</div>
          <details v-if="report.failures.length">
            <summary>{{ t('import.report.failures') }}</summary>
            <ul class="fails">
              <li v-for="(f, i) in report.failures.slice(0, FAILURES_SHOWN)" :key="i">
                <span class="num">{{ f.location }}</span> · {{ f.reason }}
              </li>
            </ul>
            <div v-if="report.failed > FAILURES_SHOWN" class="muted">
              {{ t('import.report.more', { n: report.failed - FAILURES_SHOWN }) }}
            </div>
          </details>
        </template>
        <template v-else-if="failure">
          <div v-if="failure.cancelled" class="notice info">{{ t('import.report.cancelled') }}</div>
          <div v-else class="notice error" data-test="report-error">
            <b>{{ failure.title }}</b>
            <span>{{ failure.hint }}</span>
            <span>{{ t('import.report.failedTitle') }}</span>
            <details v-if="failure.error.detail">
              <summary>{{ t('common.details') }}</summary>
              <code class="detail">{{ failure.error.detail }}</code>
            </details>
          </div>
        </template>
      </template>

      <div v-if="startError" class="notice error" data-test="start-error">
        <b>{{ startError.title }}</b>
        <span>{{ startError.hint }}</span>
        <details v-if="startError.detail">
          <summary>{{ t('common.details') }}</summary>
          <code class="detail">{{ startError.detail }}</code>
        </details>
      </div>

      <!-- 底部按钮 -->
      <div class="row foot">
        <button v-if="step === 'method' || step === 'prepare'" class="btn" @click="back">
          {{ t('common.back') }}
        </button>
        <span class="spacer" />
        <template v-if="step === 'prepare' && method === 'url'">
          <button class="btn primary" :disabled="!canStartUrl || discovering" data-test="start-url" @click="startUrl">
            {{ t('import.url.start') }}
          </button>
        </template>
        <template v-else-if="step === 'prepare' && method === 'token' && !roles">
          <button
            class="btn primary"
            :disabled="!pastedToken.trim() || loadingRoles"
            data-test="query-roles"
            @click="loadRoles({ kind: 'pasted', token: pastedToken.trim() })"
          >
            {{ t('import.hg.queryRoles') }}
          </button>
        </template>
        <template v-else-if="step === 'prepare' && isToken && roles">
          <button class="btn primary" :disabled="chosenRoles.size === 0" data-test="start-token" @click="startToken">
            {{ t('import.hg.start', { n: chosenRoles.size }) }}
          </button>
        </template>
        <template v-else-if="step === 'prepare' && method === 'file' && preview">
          <button class="btn primary" :disabled="selected.size === 0" data-test="start-file" @click="startFile">
            {{ selected.size ? t('import.file.start', { n: selected.size }) : t('import.file.noneSelected') }}
          </button>
        </template>
        <template v-else-if="step === 'progress'">
          <button class="btn" :disabled="cancelling" data-test="cancel-task" @click="cancel">
            {{ t('import.progress.cancel') }}
          </button>
        </template>
        <template v-else-if="step === 'report'">
          <button v-if="report && report.accounts.length" class="btn" data-test="view-account" @click="viewAccount">
            {{ t('import.report.viewAccount') }}
          </button>
          <button class="btn primary" @click="emit('close')">{{ t('common.done') }}</button>
        </template>
      </div>
    </div>
  </div>
</template>

<style scoped>
.steps {
  display: flex;
  gap: 4px;
  list-style: none;
  padding: 0;
  margin: 0;
  font-size: 12px;
  color: var(--text2);
  flex-wrap: wrap;
}
.steps li {
  display: flex;
  align-items: center;
  gap: 6px;
  padding-right: 12px;
}
.steps .sn {
  width: 20px;
  height: 20px;
  border-radius: 50%;
  border: 1px solid var(--border);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 11px;
}
.steps li.on {
  color: var(--primary);
  font-weight: 600;
}
.steps li.on .sn,
.steps li.done .sn {
  background: var(--primary);
  border-color: var(--primary);
  color: var(--onprimary);
}
.ggrid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 10px;
}
.gcard,
.mcard {
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text);
  border-radius: 8px;
  padding: 14px;
  display: flex;
  align-items: center;
  gap: 10px;
  cursor: pointer;
  font: inherit;
  text-align: left;
}
.gcard:hover:not(:disabled),
.mcard:hover:not(:disabled) {
  border-color: var(--primary);
  background: var(--hover);
}
.gcard.auto {
  border-style: dashed;
}
.mgrid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}
.mcard {
  flex-direction: column;
  align-items: flex-start;
  gap: 6px;
}
.mcard:disabled {
  opacity: 0.55;
  cursor: default;
}
.drop {
  border: 2px dashed var(--border);
  border-radius: 8px;
  padding: 40px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  color: var(--text2);
}
.drop.over {
  border-color: var(--primary);
  background: var(--selected);
}
.indeterminate {
  width: 30%;
  animation: slide 1.2s ease-in-out infinite;
}
@keyframes slide {
  from {
    transform: translateX(-100%);
  }
  to {
    transform: translateX(340%);
  }
}
@media (prefers-reduced-motion: reduce) {
  .indeterminate {
    animation: none;
    width: 100%;
    opacity: 0.4;
  }
}
.rgrid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 10px;
}
.rgrid .card {
  gap: 2px;
}
.big {
  font-size: 24px;
  font-weight: 600;
}
.bad {
  color: var(--loss);
}
.fails {
  margin: 8px 0 0;
  padding-left: 18px;
  font-size: 12px;
  max-height: 200px;
  overflow-y: auto;
}
.detail {
  display: block;
  white-space: pre-wrap;
  word-break: break-all;
  font-size: 12px;
  margin-top: 4px;
}
.foot {
  border-top: 1px solid var(--border);
  padding-top: 16px;
}
.tablebox {
  max-height: 320px;
}
.guide {
  margin: 0;
  padding-left: 20px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 13px;
}
</style>
