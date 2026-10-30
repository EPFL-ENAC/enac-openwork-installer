<script setup lang="ts">
import ToolInstallScreen from '../../components/ToolInstallScreen.vue'
import { installOpencode, opencodeStatus } from '../../lib/ipc'
</script>

<template>
  <ToolInstallScreen
    i18n-prefix="opencode"
    :status="opencodeStatus"
    :install="installOpencode"
    log-task="opencode"
  />
</template>
