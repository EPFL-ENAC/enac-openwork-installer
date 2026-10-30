<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { EpflButton } from '../../kit'
import {
  onPortalLogin,
  portalClose,
  portalCreateKey,
  portalGetForm,
  portalStartLogin,
  type AppError,
  type PortalLogin,
} from '../../lib/ipc'
import { chooseKeyBranch, DEFAULT_KEY_NAME } from '../../lib/key-choice'

const portalKeysUrl = 'https://portal.rcp.epfl.ch/aiaas/keys'
const manualSteps = ['step1', 'step2', 'step3'] as const

const emit = defineEmits<{
  done: [payload: { keyName: string; token: string }]
  'open-url': [url: string]
}>()

const { t } = useI18n()

const phase = ref<'login' | 'form' | 'key' | 'done'>('login')
const busy = ref(false)
const waitingForLogin = ref(false)
const error = ref<AppError | null>(null)
const notice = ref<string | null>(null)
const units = ref<string[]>([])
const existingOpenWorkKey = ref(false)
const refused = ref(false)
const chosenUnit = ref('')
const keyNameInput = ref(DEFAULT_KEY_NAME)
const keyName = ref(DEFAULT_KEY_NAME)
const token = ref<string | null>(null)
const tokenVisible = ref(false)
const tokenCopied = ref(false)
const pastedToken = ref('')

const choice = computed(() =>
  chooseKeyBranch({
    units: units.value,
    existingOpenWorkKey: existingOpenWorkKey.value,
    refused: refused.value,
    chosenUnit: chosenUnit.value,
  }),
)
const showUnit = computed(() => phase.value === 'form' || phase.value === 'key')
const unitMissing = computed(() => choice.value.showUnitPicker && choice.value.unit === null)
const canCreate = computed(() => !busy.value && keyNameInput.value.trim().length > 0 && !unitMissing.value)
const tokenMask = computed(() => '*'.repeat(Math.min(token.value?.length ?? 0, 24)))

const toAppError = (cause: unknown): AppError => {
  const failure = cause as Partial<AppError> | null
  return {
    kind: typeof failure?.kind === 'string' ? failure.kind : 'unknown',
    message: typeof failure?.message === 'string' ? failure.message : String(cause),
  }
}

const loadForm = async () => {
  busy.value = true
  error.value = null
  try {
    const form = await portalGetForm()
    units.value = form.units ?? []
    existingOpenWorkKey.value = form.existingOpenWorkKey
    phase.value = existingOpenWorkKey.value ? 'key' : 'form'
    keyNameInput.value = choice.value.suggestedKeyName
  } catch (cause) {
    error.value = toAppError(cause)
  } finally {
    busy.value = false
  }
}

const applyLogin = async (result: PortalLogin) => {
  waitingForLogin.value = false
  if (!result.ok) {
    error.value = { kind: 'portal-auth', message: result.error ?? '' }
    return
  }
  await loadForm()
}

const startLogin = async () => {
  notice.value = null
  error.value = null
  waitingForLogin.value = false
  busy.value = true
  try {
    await portalStartLogin()
    waitingForLogin.value = true
  } catch (cause) {
    error.value = toAppError(cause)
  } finally {
    busy.value = false
  }
}

const createKey = async () => {
  notice.value = null
  if (!keyNameInput.value.trim()) {
    notice.value = 'rcpKey.error.nameRequired'
    return
  }
  if (unitMissing.value) {
    notice.value = 'rcpKey.error.unitRequired'
    return
  }
  busy.value = true
  error.value = null
  try {
    const created = await portalCreateKey(keyNameInput.value.trim(), choice.value.unit)
    keyName.value = created.name
    token.value = created.token
    phase.value = 'done'
  } catch (cause) {
    const failure = toAppError(cause)
    error.value = failure
    if (failure.kind === 'portal-refused') {
      refused.value = true
      phase.value = 'key'
      keyNameInput.value = choice.value.suggestedKeyName
    }
  } finally {
    busy.value = false
  }
}

const acceptToken = () => {
  notice.value = null
  const value = pastedToken.value.trim()
  if (!value) {
    notice.value = 'rcpKey.error.tokenRequired'
    return
  }
  token.value = value
  keyName.value = keyNameInput.value.trim() || DEFAULT_KEY_NAME
  phase.value = 'done'
}

const copyToken = async () => {
  if (!token.value) return
  try {
    await navigator.clipboard.writeText(token.value)
    tokenCopied.value = true
  } catch (cause) {
    error.value = toAppError(cause)
  }
}

const openPortal = () => emit('open-url', portalKeysUrl)
const finish = () => {
  if (token.value) emit('done', { keyName: keyName.value, token: token.value })
}

let unlistenLogin: (() => void) | null = null

onMounted(async () => {
  try {
    unlistenLogin = await onPortalLogin((result) => {
      void applyLogin(result)
    })
  } catch {
    // No event bridge outside the Tauri shell. The manual fallback stays usable.
  }
  await startLogin()
})

onBeforeUnmount(() => {
  unlistenLogin?.()
  portalClose().catch(() => {})
})
</script>

<template>
  <section class="rcp-key">
    <h2>{{ t('rcpKey.title') }}</h2>
    <p class="intro">{{ t('rcpKey.intro') }}</p>

    <p v-if="error && error.kind !== 'portal-auth'" class="epfl-alert epfl-alert-danger" role="alert">
      {{ t('rcpKey.error.generic', { kind: error.kind, message: error.message }) }}
    </p>
    <p v-if="notice" class="epfl-alert epfl-alert-warning" role="alert">{{ t(notice) }}</p>

    <div v-if="phase === 'login'" class="block">
      <p v-if="busy" role="status">{{ t('rcpKey.login.starting') }}</p>
      <p v-else-if="waitingForLogin" role="status">{{ t('rcpKey.login.waiting') }}</p>
      <p v-if="error?.kind === 'portal-auth'">{{ t('rcpKey.login.failed', { message: error.message }) }}</p>
      <div class="actions">
        <EpflButton variant="secondary" :disabled="busy" @click="startLogin">
          {{ t('rcpKey.login.restart') }}
        </EpflButton>
      </div>
    </div>

    <div v-if="showUnit" class="block">
      <div class="field">
        <span class="epfl-label">{{ t('rcpKey.unit.label') }}</span>
        <select
          v-if="choice.showUnitPicker"
          v-model="chosenUnit"
          class="epfl-select"
          :aria-label="t('rcpKey.unit.label')"
        >
          <option value="" disabled>{{ t('rcpKey.unit.placeholder') }}</option>
          <option v-for="unit in units" :key="unit" :value="unit">{{ unit }}</option>
        </select>
        <p v-else class="unit-fixed">{{ choice.unit ?? '' }}</p>
      </div>

      <template v-if="phase === 'form'">
        <div class="field">
          <label class="epfl-label" for="rcp-key-name">{{ t('rcpKey.create.nameLabel') }}</label>
          <input id="rcp-key-name" v-model="keyNameInput" class="epfl-input" type="text" />
        </div>
        <div class="actions">
          <EpflButton :disabled="!canCreate" @click="createKey">{{ t('rcpKey.create.submit') }}</EpflButton>
        </div>
      </template>

      <template v-else-if="phase === 'key'">
        <h3>{{ t('rcpKey.choice.title') }}</h3>
        <p>{{ t('rcpKey.choice.text') }}</p>
        <div class="field">
          <label class="epfl-label" for="rcp-existing-token">{{ t('rcpKey.choice.pasteLabel') }}</label>
          <input
            id="rcp-existing-token"
            v-model="pastedToken"
            class="epfl-input"
            type="password"
            autocomplete="off"
            spellcheck="false"
          />
        </div>
        <div class="actions">
          <EpflButton variant="secondary" :disabled="busy" @click="acceptToken">
            {{ t('rcpKey.choice.paste') }}
          </EpflButton>
        </div>
        <div class="field">
          <label class="epfl-label" for="rcp-new-name">{{ t('rcpKey.choice.newNameLabel') }}</label>
          <input id="rcp-new-name" v-model="keyNameInput" class="epfl-input" type="text" />
        </div>
        <div class="actions">
          <EpflButton :disabled="!canCreate" @click="createKey">{{ t('rcpKey.choice.createNew') }}</EpflButton>
        </div>
      </template>
    </div>

    <div v-if="phase === 'done'" class="block">
      <h3>{{ t('rcpKey.token.title') }}</h3>
      <p class="epfl-label">{{ t('rcpKey.token.label', { name: keyName }) }}</p>
      <p class="token" :class="{ masked: !tokenVisible }">{{ tokenVisible ? token : tokenMask }}</p>
      <div class="actions">
        <EpflButton variant="secondary" size="sm" @click="tokenVisible = !tokenVisible">
          {{ tokenVisible ? t('rcpKey.token.hide') : t('rcpKey.token.reveal') }}
        </EpflButton>
        <EpflButton variant="secondary" size="sm" :disabled="!token" @click="copyToken">
          {{ tokenCopied ? t('rcpKey.token.copied') : t('rcpKey.token.copy') }}
        </EpflButton>
      </div>
      <p v-if="tokenCopied" class="warning">{{ t('rcpKey.token.warning') }}</p>
      <div class="actions">
        <EpflButton :disabled="!token" @click="finish">{{ t('rcpKey.token.continue') }}</EpflButton>
      </div>
    </div>

    <section v-if="phase !== 'done'" class="manual">
      <h3>{{ t('rcpKey.manual.title') }}</h3>
      <p>{{ t('rcpKey.manual.intro') }}</p>
      <ol class="manual-steps">
        <li v-for="step in manualSteps" :key="step">{{ t(`rcpKey.manual.${step}`) }}</li>
      </ol>
      <div class="actions">
        <EpflButton variant="secondary" @click="openPortal">{{ t('rcpKey.manual.open') }}</EpflButton>
      </div>
      <template v-if="phase !== 'key'">
        <div class="field">
          <label class="epfl-label" for="rcp-manual-token">{{ t('rcpKey.manual.pasteLabel') }}</label>
          <input
            id="rcp-manual-token"
            v-model="pastedToken"
            class="epfl-input"
            type="password"
            autocomplete="off"
            spellcheck="false"
          />
        </div>
        <div class="actions">
          <EpflButton variant="secondary" :disabled="busy" @click="acceptToken">
            {{ t('rcpKey.manual.paste') }}
          </EpflButton>
        </div>
      </template>
    </section>
  </section>
</template>

<style scoped>
.rcp-key {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  max-width: 40rem;
}

.intro {
  margin: 0;
  color: var(--fg-muted);
}

.block,
.manual {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-4);
  border: var(--border-w) solid var(--border);
  border-radius: var(--radius);
}

.manual {
  border-style: dashed;
}

.block h3,
.manual h3 {
  margin: 0;
}

.field {
  display: flex;
  flex-direction: column;
}

.unit-fixed {
  margin: 0;
  font-weight: var(--w-bold);
}

.manual-steps {
  margin: 0;
  padding-left: var(--space-4);
}

.actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.token {
  margin: 0;
  padding: var(--space-2);
  font-family: var(--font-mono);
  font-size: var(--fs-sm);
  background: var(--bg-subtle);
  border: var(--border-w) solid var(--border);
  border-radius: var(--radius);
  word-break: break-all;
}

.token.masked {
  letter-spacing: 0.1em;
  color: var(--fg-muted);
}

.warning {
  margin: 0;
  font-size: var(--fs-sm);
  color: var(--warning-fg);
}
</style>
