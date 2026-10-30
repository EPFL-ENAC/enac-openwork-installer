<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { openUrl } from '@tauri-apps/plugin-opener'
import type { Locale } from './i18n'

const { t, locale } = useI18n()

const locales: Locale[] = ['en', 'fr']

const setLocale = (value: Locale) => {
  locale.value = value
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
      <router-view @open-url="onOpenUrl" />
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
  max-width: 76rem;
  margin: 0 auto;
  padding: var(--space-5) var(--space-4);
  box-sizing: border-box;
}
</style>
