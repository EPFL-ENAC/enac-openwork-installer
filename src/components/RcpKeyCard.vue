<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { openUrl } from '@tauri-apps/plugin-opener'
import { EpflButton } from '../kit'
import type { KeyAnswer, KeyPrompt } from '../lib/runners'
import { DEFAULT_KEY_NAME } from '../lib/key-choice'
import { PORTAL_KEYS_URL } from '../lib/links'

const props = defineProps<{ prompt: KeyPrompt }>()
const emit = defineEmits<{ submit: [answer: KeyAnswer] }>()

const { t } = useI18n()
const manualSteps = ['step1', 'step2', 'step3'] as const

const unit = ref(props.prompt.unit ?? '')
const keyName = ref(props.prompt.suggestedKeyName || DEFAULT_KEY_NAME)
const pasted = ref('')

watch(
  () => props.prompt,
  (prompt) => {
    unit.value = prompt.unit ?? ''
    keyName.value = prompt.suggestedKeyName || DEFAULT_KEY_NAME
    pasted.value = ''
  },
)

const unitMissing = computed(() => props.prompt.showUnitPicker && unit.value === '')
const canCreate = computed(() => !unitMissing.value && keyName.value.trim().length > 0)
const canPaste = computed(() => pasted.value.trim().length > 0)

const create = () => {
  if (canCreate.value) emit('submit', { keyName: keyName.value.trim(), unit: unit.value || null })
}

const useToken = () => {
  if (canPaste.value) emit('submit', { token: pasted.value.trim() })
}

const openKeys = () => openUrl(PORTAL_KEYS_URL).catch(() => {})
</script>

<template>
  <form class="rcp-card" @submit.prevent>
    <h3 v-if="prompt.manual" class="rcp-title">{{ t('rcpKey.manual.title') }}</h3>
    <h3 v-else-if="prompt.branch !== 'fresh'" class="rcp-title">{{ t('rcpKey.choice.title') }}</h3>
    <p class="rcp-text">{{ prompt.manual ? t('rcpKey.manual.intro') : t('rcpKey.choice.text') }}</p>

    <div v-if="prompt.showUnitPicker" class="field">
      <label class="epfl-label" for="rcp-unit">{{ t('rcpKey.unit.label') }}</label>
      <select id="rcp-unit" v-model="unit" class="epfl-select">
        <option value="" disabled>{{ t('rcpKey.unit.placeholder') }}</option>
        <option v-for="entry in prompt.units" :key="entry" :value="entry">{{ entry }}</option>
      </select>
    </div>

    <template v-if="prompt.manual">
      <ol class="manual-steps">
        <li v-for="entry in manualSteps" :key="entry">{{ t(`rcpKey.manual.${entry}`) }}</li>
      </ol>
      <div class="actions">
        <EpflButton variant="secondary" size="sm" @click="openKeys">
          {{ t('rcpKey.manual.open') }}
        </EpflButton>
      </div>
      <div class="field">
        <label class="epfl-label" for="rcp-manual-token">{{ t('rcpKey.manual.pasteLabel') }}</label>
        <input
          id="rcp-manual-token"
          v-model="pasted"
          class="epfl-input"
          type="password"
          autocomplete="off"
          spellcheck="false"
        />
      </div>
      <div class="actions">
        <EpflButton :disabled="!canPaste" @click="useToken">{{ t('rcpKey.manual.paste') }}</EpflButton>
      </div>
    </template>

    <template v-else>
      <template v-if="prompt.branch !== 'fresh'">
        <div class="field">
          <label class="epfl-label" for="rcp-existing-token">{{ t('rcpKey.choice.pasteLabel') }}</label>
          <input
            id="rcp-existing-token"
            v-model="pasted"
            class="epfl-input"
            type="password"
            autocomplete="off"
            spellcheck="false"
          />
        </div>
        <div class="actions">
          <EpflButton variant="secondary" :disabled="!canPaste" @click="useToken">
            {{ t('rcpKey.choice.paste') }}
          </EpflButton>
        </div>
      </template>

      <div class="field">
        <label class="epfl-label" for="rcp-new-name">
          {{ prompt.branch === 'fresh' ? t('rcpKey.create.nameLabel') : t('rcpKey.choice.newNameLabel') }}
        </label>
        <input id="rcp-new-name" v-model="keyName" class="epfl-input" type="text" />
      </div>
      <div class="actions">
        <EpflButton :disabled="!canCreate" @click="create">
          {{ prompt.branch === 'fresh' ? t('rcpKey.create.submit') : t('rcpKey.choice.createNew') }}
        </EpflButton>
      </div>
    </template>
  </form>
</template>

<style scoped>
.rcp-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-3);
  border: var(--border-w) solid var(--border);
  border-radius: var(--radius);
  background: var(--bg-subtle);
}

.rcp-title {
  margin: 0;
  font-size: var(--fs-h6);
}

.rcp-text {
  margin: 0;
  font-size: var(--fs-sm);
  color: var(--fg-muted);
}

.field {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.manual-steps {
  margin: 0;
  padding-left: var(--space-4);
  font-size: var(--fs-sm);
}

.actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
}
</style>
