<script setup lang="ts">
import { nextTick, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
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
  <section class="trace">
    <h3 class="trace-log-title">{{ t('trace.log') }}</h3>
    <!-- Single-line content on purpose: whitespace inside <pre> would render as blank lines. -->
    <pre ref="logBox" class="trace-log" role="log" aria-live="polite"><span v-for="(entry, index) in trace.entries" :key="index" class="trace-line"><span class="trace-time">{{ formatClock(entry.ts) }}</span> <span class="trace-scope">{{ entry.scope }}</span> {{ entry.message }}</span><span v-if="trace.entries.length === 0" class="trace-empty">{{ t('trace.empty') }}</span></pre>
  </section>
</template>

<style scoped>
.trace {
  display: flex;
  flex-direction: column;
  padding: var(--space-2);
  border: var(--border-w) solid var(--border);
  border-radius: var(--radius);
  background: var(--bg-subtle);
}

.trace-log-title {
  margin: 0 0 var(--space-1);
  font-size: var(--fs-xs);
  text-transform: uppercase;
  letter-spacing: 0.02em;
  font-weight: var(--w-normal);
  color: var(--fg-muted);
}

.trace-log {
  margin: 0;
  max-height: 20rem;
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
