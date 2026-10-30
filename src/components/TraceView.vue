<script setup lang="ts">
import { nextTick, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { STEP_ORDER } from '../lib/flow'
import { formatClock, trace } from '../lib/trace'

const { t } = useI18n()
const logBox = ref<HTMLElement | null>(null)

watch(
  () => trace.entries.length,
  async () => {
    await nextTick()
    if (logBox.value) logBox.value.scrollTop = logBox.value.scrollHeight
  },
)
</script>

<template>
  <section class="trace" :aria-label="t('trace.title')">
    <h2 class="epfl-eyebrow">{{ t('trace.title') }}</h2>

    <ul class="trace-steps">
      <li v-for="id in STEP_ORDER" :key="id" class="trace-step">
        <span class="trace-step-title">{{ t(`stepTitles.${id}`) }}</span>
        <span :class="['trace-status', `trace-status-${trace.stepStatuses[id]}`]">
          {{ t(`trace.status.${trace.stepStatuses[id]}`) }}
        </span>
      </li>
    </ul>

    <h3 class="trace-log-title">{{ t('trace.log') }}</h3>
    <!-- Single-line content on purpose: whitespace inside <pre> would render as blank lines. -->
    <pre ref="logBox" class="trace-log" role="log" aria-live="polite"><span v-for="(entry, index) in trace.entries" :key="index" class="trace-line"><span class="trace-time">{{ formatClock(entry.ts) }}</span> <span class="trace-scope">{{ entry.scope }}</span> {{ entry.message }}</span><span v-if="trace.entries.length === 0" class="trace-empty">{{ t('trace.empty') }}</span></pre>
  </section>
</template>

<style scoped>
.trace {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-3);
  border: var(--border-w) solid var(--border);
  border-radius: var(--radius);
  background: var(--bg-subtle);
}

.trace-steps {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.trace-step {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--space-2);
  padding-bottom: var(--space-1);
  border-bottom: var(--border-w) solid var(--border-subtle);
  font-size: var(--fs-sm);
}

.trace-step-title {
  color: var(--fg);
}

.trace-status {
  font-size: var(--fs-xs);
  font-weight: var(--w-bold);
  text-transform: uppercase;
  letter-spacing: 0.02em;
}

.trace-status-pending {
  color: var(--fg-subtle);
}

.trace-status-running {
  color: var(--info);
}

.trace-status-passed {
  color: var(--success-fg);
}

.trace-status-skipped {
  color: var(--warning-fg);
}

.trace-status-failed {
  color: var(--danger-fg);
}

.trace-log-title {
  margin: var(--space-2) 0 0;
  font-size: var(--fs-h6);
  text-transform: uppercase;
  letter-spacing: 0.02em;
  font-weight: var(--w-normal);
}

.trace-log {
  margin: 0;
  max-height: 12rem;
  overflow-y: auto;
  padding: var(--space-2);
  border: var(--border-w) solid var(--border);
  border-radius: var(--radius);
  background: var(--bg);
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
  line-height: var(--lh-normal);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.trace-line {
  display: block;
}

.trace-time {
  color: var(--fg-subtle);
}

.trace-scope {
  color: var(--tertiary);
}

.trace-empty {
  color: var(--fg-subtle);
}
</style>
