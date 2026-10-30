<script setup lang="ts">
import ToolInstallScreen from '../../components/ToolInstallScreen.vue'
import { installOpenwork, openworkStatus } from '../../lib/ipc'
</script>

<template>
  <ToolInstallScreen
    i18n-prefix="openwork"
    :status="openworkStatus"
    :install="installOpenwork"
    log-task="openwork"
  />
</template>
