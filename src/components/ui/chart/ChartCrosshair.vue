<script setup lang="ts">
import { VisCrosshair } from '@unovis/vue'

defineProps<{
  template?: string
  color?: string | ((d: unknown, i: number) => string)
}>()
</script>

<template>
  <VisCrosshair
    :template="template"
    :color="color"
  />
</template>
