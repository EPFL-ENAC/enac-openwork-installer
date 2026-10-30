<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { EpflAlert, EpflButton } from '../../kit'
import { NETWORK_TIMEOUT_MS, PORTAL_URL, VPN_DOC_URL } from '../../lib/links'
import type { StepDonePayload } from '../../lib/flow'

const emit = defineEmits<{ done: [payload?: StepDonePayload]; 'open-url': [url: string] }>()
const { t } = useI18n()

type CheckState = 'checking' | 'pass' | 'fail'
const state = ref<CheckState>('checking')

const check = async () => {
  state.value = 'checking'
  try {
    // no-cors: the opaque response is enough to prove the portal is reachable.
    await fetch(PORTAL_URL, { mode: 'no-cors', signal: AbortSignal.timeout(NETWORK_TIMEOUT_MS) })
    state.value = 'pass'
  } catch {
    state.value = 'fail'
  }
}

onMounted(check)
</script>

<template>
  <section>
    <h1>{{ t('network.title') }}</h1>
    <p>{{ t('network.intro') }}</p>

    <p v-if="state === 'checking'" role="status">
      {{ t('network.checking', { host: 'portal.rcp.epfl.ch' }) }}
    </p>

    <template v-else-if="state === 'pass'">
      <EpflAlert variant="success">{{ t('network.pass') }}</EpflAlert>
      <div class="actions">
        <EpflButton @click="emit('done')">{{ t('network.next') }}</EpflButton>
      </div>
    </template>

    <template v-else>
      <EpflAlert variant="danger">{{ t('network.fail') }}</EpflAlert>
      <div class="actions">
        <EpflButton @click="check">{{ t('network.retry') }}</EpflButton>
        <EpflButton variant="secondary" @click="emit('open-url', VPN_DOC_URL)">
          {{ t('network.vpnLink') }}
        </EpflButton>
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
