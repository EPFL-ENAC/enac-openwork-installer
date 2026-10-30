<script setup lang="ts">
import { computed } from 'vue'

const props = withDefaults(
  defineProps<{
    variant?: 'primary' | 'secondary' | 'tertiary'
    size?: '' | 'sm'
    disabled?: boolean
  }>(),
  { variant: 'primary', size: '', disabled: false },
)

const classes = computed(() => [
  'epfl-btn',
  `epfl-btn-${props.variant}`,
  props.size === 'sm' && 'epfl-btn-sm',
])
</script>

<template>
  <button :class="classes" :disabled="disabled"><slot /></button>
</template>
