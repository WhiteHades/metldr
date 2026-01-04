<script setup lang="ts">
import { VisTooltip } from '@unovis/vue'

defineProps<{
  triggers?: Record<string, (data: unknown, i: number, elements: (HTMLElement | SVGElement)[]) => string | void | HTMLElement>
}>()
</script>

<template>
  <VisTooltip
    :triggers="triggers"
    :horizontal-shift="8"
    :vertical-shift="8"
  />
</template>
