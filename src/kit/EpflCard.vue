<script setup lang="ts">
defineProps<{
  title?: string
  category?: string
  distinction?: boolean
}>()
</script>

<template>
  <article :class="['epfl-card', { 'epfl-card-distinction': distinction }]">
    <span v-if="distinction" class="epfl-card-flag" />
    <div class="epfl-card-body">
      <div v-if="category" class="epfl-card-info">{{ category }}</div>
      <h3 v-if="title" class="epfl-card-title">{{ title }}</h3>
      <div class="epfl-card-text"><slot /></div>
    </div>
  </article>
</template>
