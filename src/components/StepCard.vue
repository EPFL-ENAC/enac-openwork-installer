<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { EpflAlert, EpflButton } from '../kit'
import type { StepId } from '../lib/flow'
import { maskSecrets, trace } from '../lib/trace'
import { useFlow } from '../stores/flow'

const props = defineProps<{ id: StepId; detail?: string }>()

const { t } = useI18n()
const { results, state, failedStep, retry } = useFlow()

const status = computed(() => trace.stepStatuses[props.id])
const failedHere = computed(() => failedStep.value === props.id)
const safeDetail = computed(() => (props.detail ? maskSecrets(props.detail) : ''))

const errorText = computed(() => {
  const result = results[props.id]
  const kind = result?.kind
  const detail = maskSecrets(result?.detail ?? '')
  if (kind === 'network') return t('run.error.network')
  if (kind === 'smoke') return t('run.error.smoke', { detail })
  if (kind === 'invalid-json' || kind === 'io') return t('run.error.provider', { detail })
  if (kind === 'missing-key') return t('run.error.missingKey')
  if (kind === 'openwork-broken') return t('run.error.openworkBroken')
  return t('run.error.generic', { message: detail })
})
</script>

<template>
  <section :class="['step-card', `step-card-${status}`]">
    <header class="card-head">
      <h2 class="card-title">{{ t(`stepTitles.${id}`) }}</h2>
      <span :class="['card-status', `card-status-${status}`]">
        {{ t(`trace.status.${status}`) }}
      </span>
    </header>

    <p v-if="safeDetail" class="card-detail">{{ safeDetail }}</p>

    <EpflAlert v-if="failedHere" variant="danger">{{ errorText }}</EpflAlert>

    <div v-if="failedHere" class="card-actions">
      <EpflButton size="sm" :disabled="state === 'running'" @click="retry">
        {{ t('common.retry') }}
      </EpflButton>
      <slot name="fallback" />
    </div>

    <slot />
  </section>
</template>

<style scoped>
.step-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-3);
  border: var(--border-w) solid var(--border);
  border-left-width: 0.25rem;
  border-radius: var(--radius);
  background: var(--bg);
}

.step-card-running {
  border-left-color: var(--info);
}

.step-card-passed {
  border-left-color: var(--success-fg);
}

.step-card-skipped {
  border-left-color: var(--warning-fg);
}

.step-card-failed {
  border-left-color: var(--danger-fg);
}

.step-card-pending {
  border-left-color: var(--border);
}

.card-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--space-2);
}

.card-title {
  margin: 0;
  font-size: var(--fs-h5);
}

.card-status {
  font-size: var(--fs-xs);
  font-weight: var(--w-bold);
  text-transform: uppercase;
  letter-spacing: 0.02em;
}

.card-status-pending {
  color: var(--fg-subtle);
}

.card-status-running {
  color: var(--info);
}

.card-status-passed {
  color: var(--success-fg);
}

.card-status-skipped {
  color: var(--warning-fg);
}

.card-status-failed {
  color: var(--danger-fg);
}

.card-detail {
  margin: 0;
  font-size: var(--fs-sm);
  color: var(--fg-muted);
  overflow-wrap: anywhere;
}

.card-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-2);
}
</style>
