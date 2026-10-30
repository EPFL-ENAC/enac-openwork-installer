<!-- ATOM — Button. Thin wrapper over the .epfl-btn classes from css/epfl.css. -->
<template>
  <button :class="classes" :disabled="disabled"><slot /></button>
</template>

<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps({
  variant: { type: String, default: 'primary' }, // primary | secondary | tertiary
  size: { type: String, default: '' },           // '' | 'sm'
  disabled: { type: Boolean, default: false },
})

const classes = computed(() => [
  'epfl-btn',
  `epfl-btn-${props.variant}`,
  props.size && `epfl-btn-${props.size}`,
])
</script>
