<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { EpflButton, EpflCard } from '../../kit'
import { STEP_ORDER, type StepDonePayload } from '../../lib/flow'

const emit = defineEmits<{ done: [payload?: StepDonePayload] }>()
const { t } = useI18n()
</script>

<template>
  <section>
    <h1>{{ t('welcome.title') }}</h1>
    <p class="lead">{{ t('welcome.intro') }}</p>
    <EpflCard :title="t('welcome.overview')" class="overview">
      <ol class="step-list">
        <li v-for="id in STEP_ORDER" :key="id">{{ t(`stepTitles.${id}`) }}</li>
      </ol>
    </EpflCard>
    <div class="actions">
      <EpflButton @click="emit('done')">{{ t('welcome.start') }}</EpflButton>
    </div>
  </section>
</template>

<style scoped>
.overview {
  margin: var(--space-3) 0;
}

.step-list {
  margin: 0;
  padding-left: var(--space-4);
}

.actions {
  margin-top: var(--space-3);
}
</style>
