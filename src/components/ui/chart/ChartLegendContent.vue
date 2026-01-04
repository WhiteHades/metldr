<script setup lang="ts">
import type { ChartConfig } from './types'
import { inject, computed } from 'vue'

const props = defineProps<{
  nameKey?: string
}>()

const config = inject<ChartConfig>('chartConfig', {})

const items = computed(() => {
  return Object.entries(config).filter(([_, v]) => v.color).map(([key, value]) => ({
    key,
    label: value.label,
    color: value.color,
    icon: value.icon
  }))
})
</script>

<template>
  <div class="flex items-center justify-center gap-4 pt-3">
    <div 
      v-for="item in items" 
      :key="item.key"
      class="flex items-center gap-1.5 text-xs text-muted-foreground"
    >
      <span 
        class="w-2.5 h-2.5 rounded-sm shrink-0"
        :style="{ backgroundColor: item.color }"
      />
      <span>{{ item.label }}</span>
    </div>
  </div>
</template>
