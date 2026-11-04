<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { EpflButton } from '../kit'
import RcpKeyCard from '../components/RcpKeyCard.vue'
import OpenworkRepair from '../components/OpenworkRepair.vue'
import StepCard from '../components/StepCard.vue'
import TraceView from '../components/TraceView.vue'
import type { StepId } from '../lib/flow'
import { VPN_DOC_URL } from '../lib/links'
import { trace } from '../lib/trace'
import { useFlow } from '../stores/flow'
import SummaryScreen from './steps/SummaryScreen.vue'

const emit = defineEmits<{ 'open-url': [url: string] }>()
const { t } = useI18n()
const router = useRouter()
const { state, results, keyPrompt, answerKeys, retry } = useFlow()

const SEQUENCE = ['network', 'rcpKey', 'openwork'] as const
const total = SEQUENCE.length
const done = computed(
  () =>
    SEQUENCE.filter((id) => ['passed', 'skipped', 'warning'].includes(trace.stepStatuses[id])).length,
)
const current = computed(() => Math.min(done.value + 1, total))
const stepLabel = computed(() => t('progress.step', { current: current.value, total }))

onMounted(() => {
  if (state.value === 'idle') router.replace({ name: 'welcome' })
})

const detailOf = (id: StepId): string | undefined => {
  const result = results[id]
  if (!result || result.status === 'failed') return undefined
  if (result.status === 'warning' && result.detail) return result.detail
  if (result.version) return t('run.version', { version: result.version })
  if (result.keyName) return t('run.keyName', { name: result.keyName })
  if (result.path) return result.path
  if (result.detail) return result.detail
  return undefined
}
</script>

<template>
  <SummaryScreen v-if="state === 'done'" @open-url="emit('open-url', $event)" />

  <div v-else class="run">
    <div class="run-cards">
      <header class="run-head">
        <div class="run-head-top">
          <h1 class="run-title">{{ t('run.title') }}</h1>
          <p class="run-step">{{ stepLabel }}</p>
        </div>
        <div
          class="run-progress"
          role="progressbar"
          :aria-label="t('progress.barLabel')"
          aria-valuemin="0"
          :aria-valuemax="total"
          :aria-valuenow="done"
          :aria-valuetext="stepLabel"
        >
          <span
            v-for="(id, index) in SEQUENCE"
            :key="id"
            :class="['run-progress-seg', { 'is-done': index <= done }]"
          />
        </div>
      </header>

      <StepCard id="network" :detail="detailOf('network')">
        <template #fallback>
          <EpflButton variant="secondary" size="sm" @click="emit('open-url', VPN_DOC_URL)">
            {{ t('network.vpnLink') }}
          </EpflButton>
        </template>
      </StepCard>

      <StepCard
        id="rcpKey"
        :detail="detailOf('rcpKey')"
        :paused="trace.stepStatuses.rcpKey === 'running'"
      >
        <RcpKeyCard v-if="keyPrompt" :prompt="keyPrompt" @submit="answerKeys" />
        <p v-else-if="trace.stepStatuses.rcpKey === 'running'" class="hint" role="status">
          {{ t('rcpKey.login.waiting') }}
        </p>
      </StepCard>

      <StepCard id="openwork" :detail="detailOf('openwork')">
        <template #fallback>
          <OpenworkRepair v-if="results.openwork?.kind === 'openwork-broken'" @repaired="retry" />
        </template>
      </StepCard>
    </div>

    <aside class="run-trace">
      <TraceView />
    </aside>
  </div>
</template>

<style scoped>
.run {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 18rem;
  gap: var(--space-4);
  align-items: start;
}

.run-cards {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  min-width: 0;
}

.run-head {
  margin-bottom: var(--space-2);
}

.run-head-top {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--space-2);
}

.run-title {
  margin: 0;
  font-size: var(--fs-h3);
}

.run-step {
  margin: 0;
  font-size: var(--fs-xs);
  font-variant-numeric: tabular-nums;
  letter-spacing: 0.02em;
  color: var(--fg-muted);
}

/* The one designed element: one segment per step, red up to the current step. */
.run-progress {
  display: flex;
  gap: 2px;
  height: 4px;
  margin-top: var(--space-2);
  border-radius: var(--radius);
  overflow: hidden;
  background: var(--border-subtle);
}

.run-progress-seg {
  flex: 1;
  background: var(--border-subtle);
}

.run-progress-seg.is-done {
  background: var(--epfl-red);
}

.run-trace {
  position: sticky;
  top: var(--space-4);
}

.hint {
  margin: 0;
  font-size: var(--fs-sm);
  color: var(--fg-muted);
}

@media (max-width: 64rem) {
  .run {
    grid-template-columns: minmax(0, 1fr);
  }

  .run-trace {
    position: static;
  }
}
</style>
