<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { EpflAlert, EpflButton } from '../../kit'
import { smokeTest, type AppError, type SmokeResult } from '../../lib/ipc'
import type { StepDonePayload } from '../../lib/flow'

const emit = defineEmits<{ done: [payload?: StepDonePayload] }>()
const { t } = useI18n()

type ScreenState = 'idle' | 'running' | 'pass' | 'fail' | 'error'

const state = ref<ScreenState>('idle')
const result = ref<SmokeResult | null>(null)
const error = ref<AppError | null>(null)

const run = async () => {
  state.value = 'running'
  error.value = null
  try {
    result.value = await smokeTest()
    state.value = result.value.ok ? 'pass' : 'fail'
  } catch (cause) {
    error.value = cause as AppError
    state.value = 'error'
  }
}
</script>

<template>
  <section>
    <h1>{{ t('smokeTest.title') }}</h1>
    <p>{{ t('smokeTest.intro') }}</p>

    <div v-if="state === 'idle'" class="actions">
      <EpflButton @click="run">{{ t('smokeTest.run') }}</EpflButton>
    </div>

    <p v-else-if="state === 'running'" role="status">{{ t('smokeTest.running') }}</p>

    <template v-else-if="state === 'pass'">
      <EpflAlert variant="success">{{ t('smokeTest.pass') }}</EpflAlert>
      <p v-if="result?.detail">{{ t('smokeTest.detail') }}: {{ result.detail }}</p>
      <div class="actions">
        <EpflButton @click="emit('done', { ok: true, detail: result?.detail ?? undefined })">
          {{ t('smokeTest.next') }}
        </EpflButton>
      </div>
    </template>

    <template v-else-if="state === 'fail'">
      <EpflAlert variant="danger">{{ t('smokeTest.fail') }}</EpflAlert>
      <p v-if="result?.error || result?.detail">
        {{ t('smokeTest.detail') }}: {{ result?.error ?? result?.detail }}
      </p>
      <div class="actions">
        <EpflButton @click="run">{{ t('smokeTest.retry') }}</EpflButton>
      </div>
    </template>

    <template v-else>
      <EpflAlert variant="danger">
        {{ t('common.error', { message: error?.message ?? '' }) }}
      </EpflAlert>
      <div class="actions">
        <EpflButton @click="run">{{ t('smokeTest.retry') }}</EpflButton>
      </div>
    </template>
  </section>
</template>

<style scoped>
.actions {
  display: flex;
  gap: var(--space-2);
  margin-top: var(--space-3);
}
</style>
