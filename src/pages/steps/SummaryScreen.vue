<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { EpflButton } from '../../kit'
import { OPENWORK_DOCS_URL, OPENWORK_URL } from '../../lib/links'
import { useFlow } from '../../stores/flow'

const emit = defineEmits<{ 'open-url': [url: string] }>()
const { t } = useI18n()
const { results } = useFlow()

const toolText = (id: 'opencode' | 'openwork') => {
  const result = results[id]
  if (!result) return t('summary.notInstalled')
  if (result.status === 'skipped') return t('summary.skipped')
  return t('summary.version', { version: result.version ?? '' })
}

const configPaths = computed(() =>
  [
    { label: t('summary.opencode'), path: results.opencode?.path },
    { label: t('summary.openwork'), path: results.openwork?.path },
  ].filter((entry) => entry.path),
)

const smokeText = computed(() => {
  const result = results.smokeTest
  if (!result) return t('summary.smokeNotRun')
  return result.ok ? t('summary.smokePass') : t('summary.smokeFail')
})
</script>

<template>
  <section>
    <h1>{{ t('summary.title') }}</h1>
    <p>{{ t('summary.intro') }}</p>

    <dl class="summary-list">
      <div class="summary-row">
        <dt>{{ t('summary.opencode') }}</dt>
        <dd>{{ toolText('opencode') }}</dd>
      </div>
      <div class="summary-row">
        <dt>{{ t('summary.openwork') }}</dt>
        <dd>{{ toolText('openwork') }}</dd>
      </div>
      <div class="summary-row">
        <dt>{{ t('summary.keyName') }}</dt>
        <dd>{{ results.rcpKey?.keyName ?? t('summary.noKey') }}</dd>
      </div>
      <div class="summary-row">
        <dt>{{ t('summary.smokeTest') }}</dt>
        <dd>{{ smokeText }}</dd>
      </div>
    </dl>

    <div v-if="configPaths.length" class="paths">
      <div class="epfl-eyebrow">{{ t('summary.configPaths') }}</div>
      <ul>
        <li v-for="entry in configPaths" :key="entry.label">
          {{ entry.label }}: <code>{{ entry.path }}</code>
        </li>
      </ul>
    </div>

    <div class="actions">
      <EpflButton @click="emit('open-url', OPENWORK_URL)">{{ t('summary.launch') }}</EpflButton>
      <EpflButton variant="secondary" @click="emit('open-url', OPENWORK_DOCS_URL)">
        {{ t('summary.docsLink') }}
      </EpflButton>
    </div>
  </section>
</template>

<style scoped>
.summary-list {
  margin: var(--space-3) 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.summary-row {
  display: flex;
  gap: var(--space-3);
  padding-bottom: var(--space-2);
  border-bottom: var(--border-w) solid var(--border-subtle);
}

.summary-row dt {
  width: 14rem;
  flex-shrink: 0;
  color: var(--fg-muted);
}

.summary-row dd {
  margin: 0;
}

.paths {
  margin: var(--space-3) 0;
}

.paths ul {
  margin: 0;
  padding-left: var(--space-4);
}

.actions {
  display: flex;
  gap: var(--space-2);
  margin-top: var(--space-3);
}
</style>
