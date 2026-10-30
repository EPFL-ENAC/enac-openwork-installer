<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { STEP_ORDER, type StepId } from '../lib/flow'
import { useFlow } from '../stores/flow'

const props = defineProps<{ current: StepId }>()

const { t } = useI18n()
const router = useRouter()
const { results, isUnlocked } = useFlow()

type ItemState = 'current' | 'passed' | 'skipped' | 'todo' | 'locked'

const stateOf = (id: StepId): ItemState => {
  if (!isUnlocked(id)) return 'locked'
  const result = results[id]
  if (result?.status === 'passed') return 'passed'
  if (result?.status === 'skipped') return 'skipped'
  if (id === props.current) return 'current'
  return 'todo'
}
</script>

<template>
  <nav :aria-label="t('shell.stepsTitle')">
    <h2 class="epfl-eyebrow">{{ t('shell.stepsTitle') }}</h2>
    <ol class="stepper">
      <li v-for="(id, index) in STEP_ORDER" :key="id" :class="['step', `step-${stateOf(id)}`]">
        <button
          class="step-link"
          :disabled="stateOf(id) === 'locked'"
          :aria-current="stateOf(id) === 'current' ? 'step' : undefined"
          @click="router.push({ name: id })"
        >
          <span class="step-num">{{ index + 1 }}</span>
          <span class="step-title">{{ t(`stepTitles.${id}`) }}</span>
          <span v-if="stateOf(id) === 'passed'" class="epfl-badge epfl-badge-success">
            {{ t('shell.stepState.done') }}
          </span>
          <span v-else-if="stateOf(id) === 'skipped'" class="epfl-badge epfl-badge-warning">
            {{ t('shell.stepState.skipped') }}
          </span>
        </button>
      </li>
    </ol>
  </nav>
</template>

<style scoped>
.stepper {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.step-link {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  width: 100%;
  padding: var(--space-1) var(--space-2);
  border: var(--border-w) solid transparent;
  border-radius: var(--radius);
  background: none;
  font: inherit;
  color: var(--fg);
  text-align: left;
  cursor: pointer;
}

.step-link:disabled {
  color: var(--fg-disabled);
  cursor: not-allowed;
}

.step-link:hover:not(:disabled) {
  border-color: var(--border);
}

.step-link:focus-visible {
  outline: var(--focus-w) solid var(--primary-hover);
  outline-offset: 0.125rem;
}

.step-num {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 1.5rem;
  height: 1.5rem;
  flex-shrink: 0;
  border: var(--border-w) solid var(--border-strong);
  border-radius: var(--radius);
  font-size: var(--fs-sm);
}

.step-current .step-num {
  border-color: var(--primary);
  background: var(--primary);
  color: var(--on-primary);
  font-weight: var(--w-bold);
}

.step-current .step-link {
  border-color: var(--primary);
}

.step-passed .step-link,
.step-skipped .step-link {
  color: var(--fg-muted);
}

.step-title {
  flex: 1;
}
</style>
