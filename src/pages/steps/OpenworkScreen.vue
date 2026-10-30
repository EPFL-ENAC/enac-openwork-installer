<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import ToolInstallScreen from '../../components/ToolInstallScreen.vue'
import { EpflAlert, EpflButton } from '../../kit'
import {
  installOpenwork,
  openworkRepairState,
  openworkStateStatus,
  openworkStatus,
  type AppError,
} from '../../lib/ipc'

const { t } = useI18n()

const databases = ref<string[]>([])
const consent = ref(false)
const busy = ref(false)
const backups = ref<string[] | null>(null)
const error = ref<AppError | null>(null)

const loadState = async () => {
  try {
    const state = await openworkStateStatus()
    databases.value = state.broken ? state.databases : []
  } catch (cause) {
    error.value = cause as AppError
  }
}

const repair = async () => {
  busy.value = true
  error.value = null
  try {
    const result = await openworkRepairState()
    backups.value = result.backups
    databases.value = []
  } catch (cause) {
    error.value = cause as AppError
  } finally {
    busy.value = false
  }
}

onMounted(loadState)
</script>

<template>
  <div>
    <EpflAlert v-if="databases.length" variant="warning">
      <h2>{{ t('openwork.stateBrokenTitle') }}</h2>
      <p>{{ t('openwork.stateBrokenDetail') }}</p>
      <p>{{ t('openwork.stateBrokenCause') }}</p>
      <ul class="databases">
        <li v-for="database in databases" :key="database"><code>{{ database }}</code></li>
      </ul>
      <p>{{ t('openwork.stateBrokenFix') }}</p>
      <label class="consent">
        <input v-model="consent" type="checkbox" :disabled="busy" />
        <span>{{ t('openwork.stateConsent') }}</span>
      </label>
      <div class="actions">
        <EpflButton :disabled="!consent || busy" @click="repair">
          {{ busy ? t('openwork.stateRepairing') : t('openwork.stateRepair') }}
        </EpflButton>
      </div>
    </EpflAlert>

    <EpflAlert v-if="backups?.length" variant="success">
      {{ t('openwork.stateRepaired') }}
      <ul class="databases">
        <li v-for="backup in backups" :key="backup"><code>{{ backup }}</code></li>
      </ul>
    </EpflAlert>

    <EpflAlert v-if="error" variant="danger">
      {{ t('common.error', { message: error.message }) }}
    </EpflAlert>

    <ToolInstallScreen
      i18n-prefix="openwork"
      :status="openworkStatus"
      :install="installOpenwork"
      log-task="openwork"
    />
  </div>
</template>

<style scoped>
h2 {
  margin: 0 0 var(--space-2);
  font-size: var(--fs-h4);
}

p {
  margin: 0 0 var(--space-2);
}

.databases {
  margin: 0 0 var(--space-2);
  padding-left: var(--space-4);
  font-family: var(--font-mono);
  font-size: var(--fs-sm);
  overflow-wrap: anywhere;
}

.consent {
  display: flex;
  gap: var(--space-2);
  align-items: flex-start;
  margin-bottom: var(--space-2);
}

.actions {
  display: flex;
  gap: var(--space-2);
}
</style>
