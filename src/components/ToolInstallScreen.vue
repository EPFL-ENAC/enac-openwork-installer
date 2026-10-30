<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { listen } from '@tauri-apps/api/event'
import { EpflAlert, EpflButton } from '../kit'
import type { InstallResult, InstallStatus, BackendLog, AppError } from '../lib/ipc'
import type { StepDonePayload } from '../lib/flow'

const props = defineProps<{
  i18nPrefix: string
  status: () => Promise<InstallStatus>
  install: () => Promise<InstallResult>
  logTask: string
}>()

const emit = defineEmits<{ done: [payload?: StepDonePayload] }>()
const { t } = useI18n()

type ScreenState = 'checking' | 'installed' | 'missing' | 'installing' | 'installDone' | 'error'

const state = ref<ScreenState>('checking')
const version = ref<string | null>(null)
const path = ref<string | null>(null)
const error = ref<AppError | null>(null)
const logLines = ref<string[]>([])

let unlistenLog: (() => void) | null = null
onUnmounted(() => {
  unlistenLog?.()
  unlistenLog = null
})

const checkStatus = async () => {
  state.value = 'checking'
  error.value = null
  try {
    const status = await props.status()
    version.value = status.installed ? status.version : null
    path.value = status.path
    state.value = status.installed ? 'installed' : 'missing'
  } catch (cause) {
    error.value = cause as AppError
    state.value = 'error'
  }
}

const runInstall = async () => {
  state.value = 'installing'
  error.value = null
  logLines.value = []
  try {
    unlistenLog = await listen<BackendLog>('backend-log', (event) => {
      if (event.payload.task === props.logTask) logLines.value.push(event.payload.message)
    })
    const result = await props.install()
    version.value = result.version
    path.value = result.path
    state.value = 'installDone'
  } catch (cause) {
    error.value = cause as AppError
    state.value = 'error'
  } finally {
    unlistenLog?.()
    unlistenLog = null
  }
}

onMounted(checkStatus)
</script>

<template>
  <section>
    <h1>{{ t(`${props.i18nPrefix}.title`) }}</h1>
    <p>{{ t(`${props.i18nPrefix}.intro`) }}</p>

    <p v-if="state === 'checking'" role="status">{{ t('status.checking') }}</p>

    <template v-else-if="state === 'installed'">
      <EpflAlert variant="success">
        {{ t(`${props.i18nPrefix}.installed`, { version: version ?? '' }) }}
      </EpflAlert>
      <div class="actions">
        <EpflButton
          @click="emit('done', { status: 'skipped', version: version ?? undefined, path: path ?? undefined })"
        >
          {{ t(`${props.i18nPrefix}.skip`) }}
        </EpflButton>
      </div>
    </template>

    <template v-else-if="state === 'missing'">
      <p>{{ t(`${props.i18nPrefix}.missing`) }}</p>
      <div class="actions">
        <EpflButton @click="runInstall">{{ t(`${props.i18nPrefix}.install`) }}</EpflButton>
      </div>
    </template>

    <template v-else-if="state === 'installing'">
      <p role="status">{{ t(`${props.i18nPrefix}.installing`) }}</p>
      <div v-if="logLines.length" class="log" aria-live="polite">
        <div class="epfl-eyebrow">{{ t(`${props.i18nPrefix}.progress`) }}</div>
        <pre><code v-for="(line, index) in logLines" :key="index">{{ line }}
</code></pre>
      </div>
    </template>

    <template v-else-if="state === 'installDone'">
      <EpflAlert variant="success">
        {{ t(`${props.i18nPrefix}.installDone`, { version: version ?? '' }) }}
      </EpflAlert>
      <div class="actions">
        <EpflButton
          @click="emit('done', { status: 'passed', version: version ?? undefined, path: path ?? undefined })"
        >
          {{ t(`${props.i18nPrefix}.continue`) }}
        </EpflButton>
      </div>
    </template>

    <template v-else>
      <EpflAlert variant="danger">
        {{ t('common.error', { message: error?.message ?? '' }) }}
      </EpflAlert>
      <div class="actions">
        <EpflButton @click="checkStatus">{{ t('common.retry') }}</EpflButton>
      </div>
    </template>
  </section>
</template>

<style scoped>
.actions {
  display: flex;
  gap: var(--space-2);
  margin-top: var(--space-3);
}

.log {
  margin-top: var(--space-3);
}

.log pre {
  margin: 0;
  padding: var(--space-2);
  border: var(--border-w) solid var(--border);
  border-radius: var(--radius);
  background: var(--bg-subtle);
  max-height: 16rem;
  overflow: auto;
}
</style>
