<script setup lang="ts">
withDefaults(defineProps<{ variant?: 'info' | 'success' | 'warning' | 'danger' }>(), {
  variant: 'info',
})
</script>

<template>
  <div :class="['epfl-alert', `epfl-alert-${variant}`]" role="alert"><slot /></div>
</template>
