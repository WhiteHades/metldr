<script setup lang="ts">
import { ref, computed, nextTick, watch, onMounted } from 'vue'
import { formatTime, stripThinking } from '@/utils/text'
import { marked } from 'marked'
import { ArrowUp, Square, Trash2, Loader2 } from 'lucide-vue-next'
import type { AppChatMessage } from '@/types'

interface Props {
  chatMessages: AppChatMessage[]
  chatLoading: boolean
  chatDisabled: boolean
  isViewingEmailThread: boolean
}

const props = defineProps<Props>()
const chatInput = defineModel<string>('chatInput', { required: true })

const emit = defineEmits<{
  'send': []
  'clear': []
}>()

const viewportRef = ref<HTMLDivElement | null>(null)
const textareaRef = ref<HTMLTextAreaElement | null>(null)

const isEmpty = computed(() => !chatInput.value.trim())
const isRunning = computed(() => props.chatLoading)
const isThreadEmpty = computed(() => props.chatMessages.length === 0 && !props.chatLoading)

function focusInput() {
  textareaRef.value?.focus()
}

function scrollToBottom() {
  nextTick(() => {
    if (viewportRef.value) {
      viewportRef.value.scrollTop = viewportRef.value.scrollHeight
    }
  })
}

function autoResize() {
  if (textareaRef.value) {
    textareaRef.value.style.height = 'auto'
    textareaRef.value.style.height = Math.min(textareaRef.value.scrollHeight, 120) + 'px'
  }
}

watch(() => props.chatMessages.length, scrollToBottom)
watch(() => props.chatLoading, scrollToBottom)
watch(chatInput, autoResize)

onMounted(() => {
  autoResize()
})

function renderMarkdown(text: string): string {
  if (!text) return ''
  const cleaned = stripThinking(text)
  return marked.parseInline(cleaned) as string
}

function handleSend() {
  if (!isEmpty.value && !isRunning.value) {
    emit('send')
    nextTick(autoResize)
  }
}

function handleClear() {
  emit('clear')
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault()
    handleSend()
  }
}

defineExpose({
  viewportRef,
  textareaRef,
  focusInput
})
</script>

<template>
  <div class="chat-thread flex h-full min-h-0 flex-col bg-background">
    <div v-if="isThreadEmpty && !chatDisabled" class="flex h-full flex-col items-center justify-end px-3 pb-4">
      <div class="composer w-full">
        <div class="composer-bar">
          <textarea
            ref="textareaRef"
            v-model="chatInput"
            @keydown="handleKeydown"
            :placeholder="isViewingEmailThread ? 'Ask about this email...' : 'What do you want to know?'"
            :disabled="chatDisabled"
            rows="1"
            class="composer-input"
          />
          
          <div class="flex items-center gap-1 shrink-0">
            <button
              v-if="chatMessages.length > 0"
              @click="handleClear"
              class="w-7 h-7 rounded-md flex items-center justify-center text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-all duration-150"
              title="Clear chat"
            >
              <Trash2 :size="13" />
            </button>
            
            <div class="relative w-7 h-7">
              <button
                v-if="isRunning"
                class="absolute inset-0 rounded-md bg-destructive flex items-center justify-center text-destructive-foreground transition-all duration-200"
              >
                <Square :size="11" class="fill-current" />
              </button>
              <button
                v-else
                @click="handleSend"
                :disabled="isEmpty"
                class="absolute inset-0 rounded-md flex items-center justify-center transition-all duration-200"
                :class="isEmpty ? 'bg-muted text-muted-foreground' : 'bg-foreground text-background hover:bg-foreground/90'"
              >
                <ArrowUp :size="14" :stroke-width="2.5" />
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>

    <template v-else>
      <div 
        ref="viewportRef"
        class="chat-viewport flex flex-1 min-h-0 flex-col overflow-y-auto px-3 pt-3"
        :class="{ 'opacity-40 pointer-events-none': chatDisabled }"
      >
        <div class="space-y-3">
          <div v-if="chatDisabled" class="flex flex-col items-center justify-center h-24">
            <p class="text-xs text-muted-foreground">open an email to chat</p>
          </div>

          <template v-for="(msg, i) in chatMessages" :key="i">
            <div v-if="msg.role === 'user'" class="flex justify-end">
              <div class="user-message">
                {{ msg.content }}
              </div>
            </div>
            
            <div v-else class="flex flex-col items-start gap-1">
              <div class="assistant-message">
                <div class="chat-markdown" v-html="renderMarkdown(msg.content)"></div>
              </div>
              <div v-if="msg.timing" class="timing-badge">
                <span>{{ formatTime(msg.timing.total) }}</span>
                <span v-if="msg.timing.model">· {{ msg.timing.model }}</span>
              </div>
            </div>
          </template>
          
          <!-- typing indicator -->
          <div v-if="chatLoading" class="flex items-start gap-2">
            <div class="typing-indicator">
              <span class="dot"></span>
              <span class="dot"></span>
              <span class="dot"></span>
            </div>
          </div>
        </div>
      </div>

      <div class="shrink-0 p-3 pt-2">
        <div class="composer">
          <div class="composer-bar">
            <textarea
              ref="textareaRef"
              v-model="chatInput"
              @keydown="handleKeydown"
              :placeholder="isViewingEmailThread ? 'Ask about this email...' : 'What do you want to know?'"
              :disabled="chatDisabled"
              rows="1"
              class="composer-input"
            />
            
            <div class="flex items-center gap-1 shrink-0">
              <button
                v-if="chatMessages.length > 0"
                @click="handleClear"
                class="w-7 h-7 rounded-md flex items-center justify-center text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-all duration-150"
                title="Clear chat"
              >
                <Trash2 :size="13" />
              </button>
              
              <div class="relative w-7 h-7">
                <button
                  v-if="isRunning"
                  class="absolute inset-0 rounded-md bg-destructive flex items-center justify-center text-destructive-foreground transition-all duration-200"
                >
                  <Square :size="11" class="fill-current" />
                </button>
                <button
                  v-else
                  @click="handleSend"
                  :disabled="isEmpty"
                  class="absolute inset-0 rounded-md flex items-center justify-center transition-all duration-200"
                  :class="isEmpty ? 'bg-muted text-muted-foreground' : 'bg-foreground text-background hover:bg-foreground/90'"
                >
                  <ArrowUp :size="14" :stroke-width="2.5" />
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.composer-bar {
  display: flex;
  align-items: flex-end;
  gap: 6px;
  padding: 8px 10px;
  background: hsl(var(--muted) / 0.6);
  border-radius: 20px;
  border: 1px solid hsl(var(--border) / 0.4);
  transition: background 150ms ease, border-color 150ms ease;
}

.composer-bar:focus-within {
  background: hsl(var(--muted) / 0.8);
  border-color: hsl(var(--border) / 0.6);
}

.composer-input {
  flex: 1;
  resize: none;
  background: transparent;
  color: hsl(var(--foreground));
  font-size: 12px;
  line-height: 1.5;
  outline: none;
  border: none;
  min-height: 20px;
  max-height: 120px;
  padding: 0 4px;
}

.composer-input::placeholder {
  color: hsl(var(--muted-foreground) / 0.7);
}

.composer-input:focus {
  outline: none;
}

.chat-viewport {
  scrollbar-width: thin;
  scrollbar-color: color-mix(in oklch, var(--color-foreground) 20%, transparent) transparent;
}

.chat-viewport::-webkit-scrollbar {
  width: 6px;
}

.chat-viewport::-webkit-scrollbar-track {
  background: transparent;
}

.chat-viewport::-webkit-scrollbar-thumb {
  background: color-mix(in oklch, var(--color-foreground) 20%, transparent);
  border-radius: 3px;
  transition: background 0.2s ease;
}

.chat-viewport::-webkit-scrollbar-thumb:hover {
  background: color-mix(in oklch, var(--color-foreground) 35%, transparent);
}

.chat-markdown :deep(strong),
.chat-markdown :deep(b) {
  font-weight: 600;
}

.chat-markdown :deep(em),
.chat-markdown :deep(i) {
  font-style: italic;
}

.chat-markdown :deep(code) {
  background: hsl(var(--muted));
  padding: 0.1em 0.3em;
  border-radius: 3px;
  font-family: ui-monospace, monospace;
  font-size: 0.9em;
}

.chat-markdown :deep(a) {
  color: hsl(var(--primary));
  text-decoration: underline;
  text-underline-offset: 2px;
}

.chat-markdown :deep(p) {
  margin: 0;
}

.chat-markdown :deep(ul),
.chat-markdown :deep(ol) {
  margin: 0.4em 0;
  padding-left: 1.2em;
}

/* user message bubble */
.user-message {
  max-width: 85%;
  border-radius: 16px 16px 4px 16px;
  background: linear-gradient(135deg, hsl(var(--primary)), hsl(var(--primary) / 0.85));
  color: hsl(var(--primary-foreground));
  padding: 10px 14px;
  font-size: 12px;
  line-height: 1.5;
  box-shadow: 0 2px 8px hsl(var(--primary) / 0.25);
}

/* assistant message bubble */
.assistant-message {
  max-width: 92%;
  border-radius: 16px 16px 16px 4px;
  background: hsl(var(--muted) / 0.5);
  border: 1px solid hsl(var(--border) / 0.3);
  padding: 10px 14px;
  font-size: 12px;
  line-height: 1.6;
}

.timing-badge {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 9px;
  color: hsl(var(--muted-foreground) / 0.7);
  padding-left: 4px;
}

/* typing indicator */
.typing-indicator {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 12px 16px;
  background: hsl(var(--muted) / 0.5);
  border: 1px solid hsl(var(--border) / 0.3);
  border-radius: 16px 16px 16px 4px;
}

.typing-indicator .dot {
  width: 6px;
  height: 6px;
  background: hsl(var(--primary));
  border-radius: 50%;
  animation: bounce 1.4s infinite ease-in-out both;
}

.typing-indicator .dot:nth-child(1) {
  animation-delay: 0s;
}

.typing-indicator .dot:nth-child(2) {
  animation-delay: 0.16s;
}

.typing-indicator .dot:nth-child(3) {
  animation-delay: 0.32s;
}

@keyframes bounce {
  0%, 80%, 100% {
    transform: scale(0.6);
    opacity: 0.4;
  }
  40% {
    transform: scale(1);
    opacity: 1;
  }
}
</style>
