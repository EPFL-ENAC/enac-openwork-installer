<script setup lang="ts">
withDefaults(defineProps<{ variant?: 'info' | 'danger' }>(), {
  variant: 'info',
})
</script>

<template>
  <div :class="['epfl-alert', `epfl-alert-${variant}`]" role="alert"><slot /></div>
</template>
