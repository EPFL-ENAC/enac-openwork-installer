<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { EpflButton } from '../../kit'
import { useFlow } from '../../stores/flow'

const { t } = useI18n()
const router = useRouter()
const { state, start } = useFlow()

const begin = () => {
  if (state.value === 'idle') void start()
  router.push({ name: 'run' })
}
</script>

<template>
  <section>
    <h1>{{ t('welcome.title') }}</h1>
    <p class="lead">{{ t('welcome.intro') }}</p>
    <p class="lead">{{ t('welcome.credentials') }}</p>
    <div class="actions">
      <EpflButton :disabled="state === 'running'" @click="begin">{{ t('welcome.start') }}</EpflButton>
    </div>
  </section>
</template>

<style scoped>
.lead {
  margin: 0 0 var(--space-2);
}

.actions {
  margin-top: var(--space-3);
}
</style>
