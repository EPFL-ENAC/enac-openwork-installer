<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { opencodeStatus, type AppError } from './lib/ipc'
import type { Locale } from './i18n'

const { t, locale } = useI18n()
const locales: Locale[] = ['en', 'fr']
const status = ref(t('status.checking'))

const setLocale = (value: Locale) => {
  locale.value = value
}

onMounted(async () => {
  try {
    const result = await opencodeStatus()
    status.value = result.installed
      ? t('status.installed', { version: result.version ?? '' })
      : t('status.checking')
  } catch (error) {
    status.value = t('status.error', { message: (error as AppError).message })
  }
})
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
      <h1>{{ t('welcome.title') }}</h1>
      <p>{{ t('welcome.intro') }}</p>
      <p class="status" role="status">{{ status }}</p>
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
  width: 100%;
  max-width: 60rem;
  padding: var(--space-5) var(--space-4);
}

.status {
  margin-top: var(--space-4);
  color: var(--fg-muted);
}
</style>
