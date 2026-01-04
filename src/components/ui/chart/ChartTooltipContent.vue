<script setup lang="ts">

defineProps<{
  items?: Array<{ key: string; value: number; color?: string }>
  label?: string
  labelKey?: string
  nameKey?: string
  indicator?: 'dot' | 'line' | 'dashed'
  hideLabel?: boolean
  hideIndicator?: boolean
}>()

function formatValue(value: number): string {
  if (value >= 1000000) return `${(value / 1000000).toFixed(1)}M`
  if (value >= 1000) return `${(value / 1000).toFixed(1)}k`
  return String(Math.round(value))
}
</script>

<template>
  <div class="bg-popover text-popover-foreground border border-border rounded-lg shadow-lg p-2 min-w-[120px]">
    <div v-if="label && !hideLabel" class="text-xs text-muted-foreground mb-1.5 font-medium">
      {{ label }}
    </div>
    <div v-if="items?.length" class="space-y-1">
      <div 
        v-for="item in items" 
        :key="item.key"
        class="flex items-center justify-between gap-3"
      >
        <div class="flex items-center gap-1.5">
          <span 
            v-if="!hideIndicator"
            class="w-2 h-2 rounded-full shrink-0"
            :class="{
              'w-0.5 h-3': indicator === 'line',
              'border border-dashed bg-transparent': indicator === 'dashed'
            }"
            :style="{ backgroundColor: indicator !== 'dashed' ? item.color : undefined, borderColor: item.color }"
          />
          <span class="text-xs text-muted-foreground">{{ item.key }}</span>
        </div>
        <span class="text-xs font-medium tabular-nums">{{ formatValue(item.value) }}</span>
      </div>
    </div>
  </div>
</template>
