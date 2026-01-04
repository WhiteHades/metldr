<script setup lang="ts">
import { computed, useSlots } from 'vue'
import type { ChartConfig } from './types'

const props = withDefaults(defineProps<{
  config: ChartConfig
  cursor?: boolean
}>(), {
  cursor: false
})

const cssVars = computed(() => {
  const vars: Record<string, string> = {}
  for (const [key, value] of Object.entries(props.config)) {
    if (value.color) {
      vars[`--color-${key}`] = value.color
    }
  }
  return vars
})

const slots = useSlots()
</script>

<template>
  <div 
    class="chart-container w-full" 
    :style="cssVars"
    :class="{ 'cursor-crosshair': cursor }"
  >
    <slot />
  </div>
</template>

<style scoped>
.chart-container {
  --vis-font-family: inherit;
  --vis-color: var(--foreground);
  --vis-axis-grid-line-color: var(--muted);
  --vis-axis-tick-label-color: var(--muted-foreground);
  --vis-tooltip-background-color: var(--popover);
  --vis-tooltip-text-color: var(--popover-foreground);
  --vis-tooltip-border-color: var(--border);
  --vis-crosshair-line-color: var(--border);
}

.chart-container :deep(svg) {
  overflow: visible;
}

.chart-container :deep(.unovis-xy-container) {
  height: 100%;
}
</style>
