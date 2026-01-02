<script setup lang="ts">
import type { TooltipContentEmits, TooltipContentProps } from "reka-ui"
import type { HTMLAttributes } from "vue"
import { reactiveOmit } from "@vueuse/core"
import { TooltipArrow, TooltipContent, useForwardPropsEmits } from "reka-ui"
import { cn } from "@/utils/cn"

defineOptions({
  inheritAttrs: false,
})

const props = withDefaults(defineProps<TooltipContentProps & { class?: HTMLAttributes["class"] }>(), {
  sideOffset: 4,
})

const emits = defineEmits<TooltipContentEmits>()

const delegatedProps = reactiveOmit(props, "class")
const forwarded = useForwardPropsEmits(delegatedProps, emits)
</script>

<template>
  <TooltipContent
    data-slot="tooltip-content"
    v-bind="{ ...forwarded, ...$attrs }"
    :class="cn('bg-zinc-950 text-white border border-zinc-800 animate-in fade-in-0 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 duration-200 z-50 w-fit rounded-md px-3 py-1.5 text-xs text-balance shadow-md', props.class)"
  >
    <slot />

    <TooltipArrow class="fill-zinc-950 z-50 size-2.5 translate-y-[calc(-50%_-_2px)] rotate-45 rounded-[2px]" />
  </TooltipContent>
</template>
