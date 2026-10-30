<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  BACKUP_PATH,
  CONFIG_PATH,
  PROVIDER_ID,
  displayPath,
  writeProviderConfig,
  type WriteOutcome,
} from '../../lib/provider-config'

const emit = defineEmits<{ done: [] }>()
const { t } = useI18n()

const configPath = displayPath(CONFIG_PATH)
const backupPath = displayPath(BACKUP_PATH)

const busy = ref(false)
const outcome = ref<WriteOutcome | null>(null)
const written = ref(false)

const message = computed(() => {
  const result = outcome.value
  if (!result) return null
  if (!result.ok) {
    return {
      kind: 'error' as const,
      text: t(result.code === 'invalid-json' ? 'steps.providerConfig.invalidJson' : 'steps.providerConfig.ioError', {
        detail: result.detail,
      }),
    }
  }
  if (!result.changed) return { kind: 'info' as const, text: t('steps.providerConfig.unchanged') }
  return {
    kind: 'success' as const,
    text: [
      t('steps.providerConfig.success'),
      result.backupCreated && t('steps.providerConfig.backupCreated'),
    ]
      .filter(Boolean)
      .join(' '),
  }
})

const run = async () => {
  busy.value = true
  try {
    const result = await writeProviderConfig()
    outcome.value = result
    if (result.ok && !written.value) {
      written.value = true
      emit('done')
    }
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <section class="step">
    <h2 class="step-title">{{ t('steps.providerConfig.title') }}</h2>
    <p class="step-intro">{{ t('steps.providerConfig.intro') }}</p>

    <dl class="paths">
      <dt>{{ t('steps.providerConfig.pathsLabel') }}</dt>
      <dd>
        <span class="path-label">{{ t('steps.providerConfig.configPath') }}</span>
        <code class="path">{{ configPath }}</code>
      </dd>
      <dd>
        <span class="path-label">{{ t('steps.providerConfig.backupPath') }}</span>
        <code class="path">{{ backupPath }}</code>
      </dd>
    </dl>

    <p class="provider">{{ PROVIDER_ID }}</p>

    <div class="actions">
      <q-btn
        color="primary"
        :label="written ? t('steps.providerConfig.rerun') : t('steps.providerConfig.write')"
        :loading="busy"
        :disable="busy"
        no-caps
        @click="run"
      />
    </div>

    <p v-if="busy" class="message" role="status">{{ t('steps.providerConfig.writing') }}</p>
    <p
      v-else-if="message"
      :class="['message', `message-${message.kind}`]"
      :role="message.kind === 'error' ? 'alert' : 'status'"
    >
      {{ message.text }}
    </p>

    <p class="hint">{{ t('steps.providerConfig.keyHint') }}</p>
  </section>
</template>

<style scoped>
.step {
  max-width: 46rem;
}

.step-title {
  margin: 0 0 var(--space-2);
  font-size: var(--fs-h4);
  font-weight: var(--w-bold);
}

.step-intro {
  margin: 0 0 var(--space-4);
  color: var(--fg-muted);
}

.paths {
  margin: 0 0 var(--space-3);
  padding: var(--space-3);
  border: var(--border-w) solid var(--border);
  border-radius: var(--radius);
  background: var(--bg-subtle);
}

.paths dt {
  font-weight: var(--w-bold);
  margin-bottom: var(--space-2);
}

.paths dd {
  margin: 0 0 var(--space-2);
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.paths dd:last-child {
  margin-bottom: 0;
}

.path-label {
  font-size: var(--fs-sm);
  color: var(--fg-muted);
}

.path {
  font-family: var(--font-mono);
  font-size: var(--fs-sm);
  overflow-wrap: anywhere;
}

.provider {
  margin: 0 0 var(--space-3);
  font-family: var(--font-mono);
  font-size: var(--fs-sm);
  color: var(--fg-muted);
}

.actions {
  margin-bottom: var(--space-3);
}

.message {
  margin: 0 0 var(--space-3);
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius);
  border: var(--border-w) solid var(--border);
}

.message-success {
  background: var(--success-bg);
  border-color: var(--success-border);
  color: var(--success-fg);
}

.message-info {
  background: var(--info-bg);
  border-color: var(--info-border);
  color: var(--info-fg);
}

.message-error {
  background: var(--danger-bg);
  border-color: var(--danger-border);
  color: var(--danger-fg);
}

.hint {
  margin: 0;
  font-size: var(--fs-sm);
  color: var(--fg-muted);
}
</style>
