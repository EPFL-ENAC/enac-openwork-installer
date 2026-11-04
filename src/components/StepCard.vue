<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { EpflAlert, EpflButton } from '../kit'
import type { StepId } from '../lib/flow'
import { maskSecrets, trace } from '../lib/trace'
import { useFlow } from '../stores/flow'

const props = defineProps<{ id: StepId; detail?: string; paused?: boolean }>()

const { t } = useI18n()
const { results, state, failedStep, retry } = useFlow()

const status = computed(() => trace.stepStatuses[props.id])
const active = computed(() => status.value === 'running')
const busy = computed(() => active.value && !props.paused)
const failedHere = computed(() => failedStep.value === props.id)
const safeDetail = computed(() => (props.detail ? maskSecrets(props.detail) : ''))

const errorText = computed(() => {
  const result = results[props.id]
  if (!result) return ''
  const kind = result.kind
  const detail = maskSecrets(result.detail ?? '')
  if (kind === 'network') return t('run.error.network')
  if (kind === 'missing-key') return t('run.error.missingKey')
  if (kind === 'openwork-broken') return t('run.error.openworkBroken')
  if (!detail.trim()) return ''
  if (kind === 'smoke') return t('opencode.error.smoke', { detail })
  if (kind === 'invalid-json' || kind === 'io') return t('opencode.error.provider', { detail })
  return t('run.error.generic', { message: detail })
})
</script>

<template>
  <section :class="['step-card', `step-card-${status}`]" :aria-busy="busy ? 'true' : undefined">
    <header class="card-head">
      <h2 class="card-title">
        <span v-if="active && !paused" class="step-spin" aria-hidden="true" />
        {{ t(`stepTitles.${id}`) }}
      </h2>
      <span :class="['card-status', `card-status-${status}`]">
        <svg
          v-if="status === 'passed'"
          class="card-status-check"
          viewBox="0 0 12 12"
          aria-hidden="true"
        >
          <path
            d="M2 6.5 4.5 9 10 3.5"
            fill="none"
            stroke="currentColor"
            stroke-width="1.6"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
        {{ t(`trace.status.${status}`) }}
      </span>
    </header>

    <p v-if="active && paused" class="card-waiting" role="status">
      {{ t('progress.waiting') }}
    </p>

    <p v-if="safeDetail" class="card-detail">{{ safeDetail }}</p>

    <EpflAlert v-if="failedHere && errorText" variant="danger">{{ errorText }}</EpflAlert>

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
  border-left-color: var(--epfl-red);
}

.step-card-running .card-title {
  color: var(--epfl-red);
}

.step-card-passed {
  border-left-color: var(--fg-muted);
}

.step-card-skipped {
  border-left-color: var(--border-subtle);
}

.step-card-warning {
  border-left-color: var(--tertiary);
}

.step-card-failed {
  border-left-color: var(--primary);
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
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin: 0;
  font-size: var(--fs-h5);
}

.step-spin {
  flex: none;
  width: 0.75rem;
  height: 0.75rem;
  border: 2px solid var(--epfl-red);
  border-top-color: transparent;
  border-radius: var(--radius-pill);
  animation: step-turn 0.9s linear infinite;
}

@keyframes step-turn {
  to {
    transform: rotate(360deg);
  }
}

/* A stopped clock still tells the time: keep the ring, drop the turn. */
@media (prefers-reduced-motion: reduce) {
  .step-spin {
    animation: none;
    border-top-color: var(--epfl-red);
  }
}

.card-waiting {
  margin: 0;
  font-size: var(--fs-sm);
  color: var(--fg-muted);
}

.card-status {
  font-size: var(--fs-xs);
  font-weight: var(--w-bold);
  text-transform: uppercase;
  letter-spacing: 0.02em;
}

.card-status-check {
  width: 0.75rem;
  height: 0.75rem;
  margin-right: var(--space-1);
  vertical-align: -0.125rem;
}

.card-status-pending {
  color: var(--fg-subtle);
}

.card-status-running {
  color: var(--fg-muted);
}

.card-status-passed {
  color: var(--fg-muted);
}

.card-status-skipped {
  color: var(--fg-disabled);
}

.card-status-warning {
  color: var(--tertiary);
}

.card-status-failed {
  color: var(--primary);
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
