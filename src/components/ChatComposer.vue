<script setup lang="ts">
import { ref, watch, onMounted, computed } from 'vue'
import { ArrowUp, Square, Trash2, Loader2 } from 'lucide-vue-next'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui'

interface Props {
  placeholder?: string
  disabled?: boolean
  loading?: boolean
  showClear?: boolean
  showStop?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  placeholder: 'ask anything...',
  disabled: false,
  loading: false,
  showClear: false,
  showStop: false
})

const modelValue = defineModel<string>({ required: true })

const emit = defineEmits<{
  'send': []
  'clear': []
  'stop': []
}>()

const textareaRef = ref<HTMLTextAreaElement | null>(null)

const isEmpty = computed(() => !modelValue.value.trim())

function autoResize() {
  if (textareaRef.value) {
    const textarea = textareaRef.value
    // reset to min height to get accurate scrollHeight
    textarea.style.height = '20px'
    // set to scrollHeight (capped at max)
    textarea.style.height = Math.min(textarea.scrollHeight, 120) + 'px'
  }
}

watch(modelValue, autoResize)

onMounted(() => {
  autoResize()
})

function handleSend() {
  if (!isEmpty.value && !props.loading && !props.disabled) {
    emit('send')
  }
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault()
    handleSend()
  }
}

function focus() {
  textareaRef.value?.focus()
}

defineExpose({ focus, textareaRef })
</script>

<template>
  <div class="composer" :class="{ 'composer-disabled': disabled }">
    <div class="composer-bar">
      <textarea
        ref="textareaRef"
        v-model="modelValue"
        @keydown="handleKeydown"
        :placeholder="placeholder"
        :disabled="disabled"
        rows="1"
        class="composer-input"
        :class="{ 'cursor-not-allowed opacity-50': disabled }"
      />
      
      <div class="flex items-center gap-1 shrink-0">
        <TooltipProvider v-if="showClear && !loading">
          <Tooltip>
            <TooltipTrigger as-child>
              <button
                @click="emit('clear')"
                class="w-7 h-7 rounded-md flex items-center justify-center text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-all duration-150"
              >
                <Trash2 :size="13" />
              </button>
            </TooltipTrigger>
            <TooltipContent>clear chat</TooltipContent>
          </Tooltip>
        </TooltipProvider>
        
        <div class="relative w-7 h-7">
          <button
            v-if="showStop && loading"
            @click="emit('stop')"
            class="absolute inset-0 rounded-md bg-destructive flex items-center justify-center text-destructive-foreground transition-all duration-200"
          >
            <Square :size="11" class="fill-current" />
          </button>
          <button
            v-else
            @click="handleSend"
            :disabled="isEmpty || disabled"
            class="absolute inset-0 rounded-md flex items-center justify-center transition-all duration-200"
            :class="(isEmpty || disabled) ? 'bg-muted text-muted-foreground cursor-not-allowed' : 'bg-foreground text-background hover:bg-foreground/90'"
          >
            <Loader2 v-if="loading" :size="14" class="animate-spin" />
            <ArrowUp v-else :size="14" :stroke-width="2.5" />
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.composer-bar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 10px;
  background: color-mix(in oklch, var(--color-card) 60%, transparent);
  border-radius: 20px;
  border: 1px solid color-mix(in oklch, var(--color-border) 40%, transparent);
  transition: background 150ms ease, border-color 150ms ease;
}

.composer-bar:focus-within {
  background: color-mix(in oklch, var(--color-card) 80%, transparent);
  border-color: color-mix(in oklch, var(--color-border) 60%, transparent);
}

.composer-disabled .composer-bar {
  background: color-mix(in oklch, var(--color-muted) 40%, transparent);
  border-color: color-mix(in oklch, var(--color-border) 30%, transparent);
}

.composer-input {
  flex: 1;
  resize: none;
  background: transparent;
  color: var(--color-foreground);
  font-size: var(--font-text-body);
  line-height: 1.5;
  outline: none;
  border: none;
  min-height: 20px;
  max-height: 120px;
  padding: 0 4px;
  font-family: inherit;
}

.composer-input::placeholder {
  color: color-mix(in oklch, var(--color-muted-foreground) 70%, transparent);
}

.composer-input:focus {
  outline: none;
}
</style>
