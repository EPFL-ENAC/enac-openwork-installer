<script setup lang="ts">
import { onMounted } from 'vue'
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

onMounted(() => {
  if (state.value === 'idle') router.replace({ name: 'welcome' })
})

const detailOf = (id: StepId): string | undefined => {
  const result = results[id]
  if (!result || result.status === 'failed') return undefined
  if (result.version) return t('run.version', { version: result.version })
  if (result.keyName) return t('run.keyName', { name: result.keyName })
  if (result.path) return result.path
  if (result.detail) return result.detail
  return undefined
}
</script>

<template>
  <div class="run">
    <div class="run-cards">
      <header class="run-head">
        <h1 class="run-title">{{ t('run.title') }}</h1>
        <p class="run-intro">{{ t('run.intro') }}</p>
      </header>

      <StepCard id="network" :detail="detailOf('network')">
        <template #fallback>
          <EpflButton variant="secondary" size="sm" @click="emit('open-url', VPN_DOC_URL)">
            {{ t('network.vpnLink') }}
          </EpflButton>
        </template>
      </StepCard>

      <StepCard id="rcpKey" :detail="detailOf('rcpKey')">
        <RcpKeyCard v-if="keyPrompt" :prompt="keyPrompt" @submit="answerKeys" />
        <p v-else-if="trace.stepStatuses.rcpKey === 'running'" class="hint" role="status">
          {{ t('rcpKey.login.waiting') }}
        </p>
      </StepCard>

      <StepCard id="opencode" :detail="detailOf('opencode')" />

      <StepCard id="openwork" :detail="detailOf('openwork')">
        <template #fallback>
          <OpenworkRepair v-if="results.openwork?.kind === 'openwork-broken'" @repaired="retry" />
        </template>
      </StepCard>

      <section v-if="state === 'done'" class="run-summary">
        <SummaryScreen @open-url="emit('open-url', $event)" />
      </section>
    </div>

    <aside class="run-trace">
      <TraceView />
    </aside>
  </div>
</template>

<style scoped>
.run {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 22rem;
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

.run-title {
  margin: 0 0 var(--space-1);
  font-size: var(--fs-h3);
}

.run-intro {
  margin: 0;
  color: var(--fg-muted);
}

.run-trace {
  position: sticky;
  top: var(--space-4);
}

.run-summary {
  margin-top: var(--space-3);
  padding-top: var(--space-3);
  border-top: var(--border-w) solid var(--border);
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
