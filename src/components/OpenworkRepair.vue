<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { EpflAlert, EpflButton } from '../kit'
import { openworkRepairState, type AppError } from '../lib/ipc'

const emit = defineEmits<{ repaired: [] }>()
const { t } = useI18n()

const consent = ref(false)
const busy = ref(false)
const error = ref<AppError | null>(null)

const repair = async () => {
  busy.value = true
  error.value = null
  try {
    await openworkRepairState()
    emit('repaired')
  } catch (cause) {
    error.value = cause as AppError
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="repair">
    <h3 class="repair-title">{{ t('openwork.stateBrokenTitle') }}</h3>
    <p>{{ t('openwork.stateBrokenDetail') }}</p>
    <p>{{ t('openwork.stateBrokenCause') }}</p>
    <p>{{ t('openwork.stateBrokenFix') }}</p>
    <label class="consent">
      <input v-model="consent" type="checkbox" :disabled="busy" />
      <span>{{ t('openwork.stateConsent') }}</span>
    </label>
    <EpflAlert v-if="error && error.message.trim()" variant="danger">
      {{ t('common.error', { message: error.message }) }}
    </EpflAlert>
    <div class="actions">
      <EpflButton :disabled="!consent || busy" @click="repair">
        {{ busy ? t('openwork.stateRepairing') : t('openwork.stateRepair') }}
      </EpflButton>
    </div>
  </div>
</template>

<style scoped>
.repair {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-3);
  border: var(--border-w) solid var(--warning-border);
  border-radius: var(--radius);
  background: var(--warning-bg);
  font-size: var(--fs-sm);
}

.repair-title {
  margin: 0;
  font-size: var(--fs-h6);
}

p {
  margin: 0;
}

.consent {
  display: flex;
  gap: var(--space-2);
  align-items: flex-start;
}

.actions {
  display: flex;
  gap: var(--space-2);
}
</style>
