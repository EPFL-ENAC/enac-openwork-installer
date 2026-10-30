<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { openUrl } from '@tauri-apps/plugin-opener'
import { EpflAlert } from './kit'
import type { Locale } from './i18n'
import { isStepRoute, router } from './router'
import { nextStep, type StepDonePayload, type StepId } from './lib/flow'
import { useFlow } from './stores/flow'
import StepStepper from './components/StepStepper.vue'

const { t, locale } = useI18n()
const route = useRoute()
const { completeStep, isUnlocked } = useFlow()

const locales: Locale[] = ['en', 'fr']
const currentStep = computed<StepId>(() => (isStepRoute(route.name) ? route.name : 'welcome'))
const stepLocked = computed(() => !isUnlocked(currentStep.value))

const setLocale = (value: Locale) => {
  locale.value = value
}

const onDone = (payload?: StepDonePayload) => {
  completeStep(currentStep.value, payload)
  const next = nextStep(currentStep.value)
  if (next) router.push({ name: next })
}

const onOpenUrl = (url: string) => {
  openUrl(url).catch(() => {})
}
</script>

<template>
  <div class="app">
    <header class="app-header">
      <span class="app-title">{{ t('app.title') }}</span>
      <div class="locale-switch" :aria-label="t('locale.label')">
        <q-btn
          v-for="code in locales"
          :key="code"
          :label="t(`locale.${code}`)"
          :outline="locale !== code"
          dense
          flat
          class="locale-btn"
          @click="setLocale(code)"
        />
      </div>
    </header>

    <main class="app-main">
      <StepStepper :current="currentStep" class="app-stepper" />
      <div class="app-content">
        <EpflAlert v-if="stepLocked" variant="warning">{{ t('shell.locked') }}</EpflAlert>
        <router-view @done="onDone" @open-url="onOpenUrl" />
      </div>
    </main>
  </div>
</template>

<style scoped>
.app {
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  background: var(--bg);
  color: var(--fg);
}

.app-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  padding: var(--space-2) var(--space-4);
  border-bottom: var(--border-w) solid var(--border);
}

.app-title {
  font-weight: var(--w-bold);
}

.locale-switch {
  display: flex;
  gap: var(--space-1);
}

.locale-btn {
  border-radius: var(--radius);
}

.app-main {
  flex: 1;
  display: flex;
  align-items: flex-start;
  gap: var(--space-5);
  width: 100%;
  max-width: 60rem;
  margin: 0 auto;
  padding: var(--space-5) var(--space-4);
  box-sizing: border-box;
}

.app-stepper {
  width: 14rem;
  flex-shrink: 0;
}

.app-content {
  flex: 1;
  min-width: 0;
}

@media (max-width: 767.98px) {
  .app-main {
    flex-direction: column;
    gap: var(--space-3);
  }

  .app-stepper {
    width: 100%;
  }
}
</style>
