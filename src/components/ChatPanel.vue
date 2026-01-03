<script setup lang="ts">
import { ref, computed, nextTick, watch, onMounted } from 'vue'
import { formatTime, stripThinking } from '@/utils/text'
import { marked } from 'marked'
import { ArrowUp, Square, Trash2, Loader2, MessageSquare } from 'lucide-vue-next'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui'
import type { AppChatMessage } from '@/types'

interface Props {
  chatMessages: AppChatMessage[]
  chatLoading: boolean
  chatIndexing?: boolean
  summaryLoading?: boolean
  chatDisabled: boolean
  disabledReason?: string
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

const isIndexingOrProcessing = computed(() => props.chatIndexing || props.summaryLoading)
const inputDisabled = computed(() => props.chatDisabled || isIndexingOrProcessing.value)
const inputPlaceholder = computed(() => {
  if (props.summaryLoading) return 'analyzing page, please wait...'
  if (props.chatIndexing) return 'indexing document, please wait...'
  return 'ask anything about this page...'
})

function focusInput() {
  textareaRef.value?.focus()
}

function scrollToBottom(smooth = false) {
  nextTick(() => {
    if (viewportRef.value) {
      viewportRef.value.scrollTo({
        top: viewportRef.value.scrollHeight,
        behavior: smooth ? 'smooth' : 'instant'
      })
    }
  })
}

function autoResize() {
  if (textareaRef.value) {
    textareaRef.value.style.height = 'auto'
    textareaRef.value.style.height = Math.min(textareaRef.value.scrollHeight, 120) + 'px'
  }
}

watch(() => props.chatMessages.length, () => scrollToBottom(true))
watch(() => props.chatLoading, () => scrollToBottom())
watch(chatInput, autoResize)

onMounted(() => {
  autoResize()
})

// configure marked for better output
marked.setOptions({
  breaks: true,
  gfm: true
})

function renderMarkdown(text: string): string {
  if (!text) return ''
  const cleaned = stripThinking(text)
  // use full parse for block-level elements (code blocks, lists, etc)
  return marked.parse(cleaned, { async: false }) as string
}

function handleSend() {
  if (!isEmpty.value && !isRunning.value && !isIndexingOrProcessing.value) {
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
    <div v-if="isThreadEmpty && !chatDisabled" class="flex h-full flex-col px-3 pb-4">
      <div class="flex-1 flex items-center justify-center">
        <div class="empty-state text-center flex flex-col items-center justify-center">
          <MessageSquare :size="48" class="text-muted-foreground/40 mb-2" stroke-width="1.5" />
        </div>
      </div>
      <div class="composer w-full">
        <div class="composer-bar">
          <textarea
            ref="textareaRef"
            v-model="chatInput"
            @keydown="handleKeydown"
            placeholder="ask anything about this page..."
            :disabled="chatDisabled"
            rows="1"
            class="composer-input"
          />
          
          <div class="flex items-center gap-1 shrink-0">
            <TooltipProvider v-if="chatMessages.length > 0">
              <Tooltip>
                <TooltipTrigger as-child>
                  <button
                    @click="handleClear"
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
          <div v-if="chatDisabled" class="flex flex-col items-center justify-center py-8 px-4 text-center">
            <template v-if="disabledReason === 'system'">
              <p class="text-xs text-muted-foreground">chat unavailable on system pages</p>
            </template>
            <template v-else-if="disabledReason === 'local-pdf'">
              <p class="text-xs text-muted-foreground">use the file picker to open the PDF first</p>
              <p class="text-xs text-muted-foreground/60 mt-1">chrome doesn't allow direct access to local files</p>
            </template>
            <template v-else>
              <p class="text-xs text-muted-foreground">open an email to chat</p>
            </template>
          </div>

          <template v-for="(msg, i) in chatMessages" :key="i">
            <div v-if="msg.role === 'user'" class="flex justify-end message-appear">
              <div class="user-message">
                {{ msg.content }}
              </div>
            </div>
            
            <div v-else class="flex flex-col items-start gap-1 message-appear">
              <div 
                class="assistant-message"
                :class="{ streaming: chatLoading && i === chatMessages.length - 1 && !msg.timing }"
              >
                <div class="chat-markdown" v-html="renderMarkdown(msg.content)"></div>
              </div>
              <div v-if="msg.timing" class="timing-badge">
                <span>{{ formatTime(msg.timing.total) }}</span>
                <span v-if="msg.timing.model">· {{ msg.timing.model }}</span>
              </div>
            </div>
          </template>
          
          <!-- indexing indicator -->
          <div v-if="chatIndexing" class="flex items-start gap-2">
            <div class="indexing-indicator">
              <Loader2 :size="14" class="animate-spin text-primary" />
              <span class="text-xs text-muted-foreground">analyzing document...</span>
            </div>
          </div>
          
          <div 
            v-if="chatLoading && !chatIndexing && !(chatMessages.length > 0 && chatMessages[chatMessages.length - 1]?.role === 'assistant' && !chatMessages[chatMessages.length - 1]?.timing)" 
            class="flex items-start gap-2"
          >
            <div class="typing-indicator">
              <span class="dot"></span>
              <span class="dot"></span>
              <span class="dot"></span>
            </div>
          </div>
        </div>
      </div>

      <div class="shrink-0 p-3 pt-2">
        <div class="composer" :class="{ 'composer-disabled': isIndexingOrProcessing }">
          <div class="composer-bar">
            <textarea
              ref="textareaRef"
              v-model="chatInput"
              @keydown="handleKeydown"
              :placeholder="inputPlaceholder"
              :disabled="inputDisabled"
              rows="1"
              class="composer-input"
              :class="{ 'cursor-not-allowed opacity-50': isIndexingOrProcessing }"
            />
            
            <div class="flex items-center gap-1 shrink-0">
              <TooltipProvider v-if="chatMessages.length > 0">
                <Tooltip>
                  <TooltipTrigger as-child>
                    <button
                      @click="handleClear"
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
                  v-if="isRunning"
                  class="absolute inset-0 rounded-md bg-destructive flex items-center justify-center text-destructive-foreground transition-all duration-200"
                >
                  <Square :size="11" class="fill-current" />
                </button>
                <button
                  v-else
                  @click="handleSend"
                  :disabled="isEmpty || isIndexingOrProcessing"
                  class="absolute inset-0 rounded-md flex items-center justify-center transition-all duration-200"
                  :class="(isEmpty || isIndexingOrProcessing) ? 'bg-muted text-muted-foreground cursor-not-allowed' : 'bg-foreground text-background hover:bg-foreground/90'"
                >
                  <Loader2 v-if="isIndexingOrProcessing" :size="14" class="animate-spin" />
                  <ArrowUp v-else :size="14" :stroke-width="2.5" />
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
}

.composer-input::placeholder {
  color: color-mix(in oklch, var(--color-muted-foreground) 70%, transparent);
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
  background: var(--color-card);
  padding: 0.1em 0.3em;
  border-radius: 3px;
  font-family: ui-monospace, monospace;
  font-size: 0.9em;
}

.chat-markdown :deep(a) {
  color: var(--color-primary);
  text-decoration: underline;
  text-underline-offset: 2px;
}

.chat-markdown :deep(p) {
  margin: 0 0 0.5em 0;
}

.chat-markdown :deep(p:last-child) {
  margin-bottom: 0;
}

.chat-markdown :deep(ul),
.chat-markdown :deep(ol) {
  margin: 0.4em 0;
  padding-left: 1.2em;
}

.chat-markdown :deep(li) {
  margin: 0.2em 0;
}

.chat-markdown :deep(h1),
.chat-markdown :deep(h2),
.chat-markdown :deep(h3) {
  font-weight: 600;
  margin: 0.6em 0 0.3em 0;
  line-height: 1.3;
}

.chat-markdown :deep(h1) { font-size: 1.2em; }
.chat-markdown :deep(h2) { font-size: 1.1em; }
.chat-markdown :deep(h3) { font-size: 1em; }

.chat-markdown :deep(pre) {
  background: color-mix(in oklch, var(--color-background) 60%, var(--color-muted));
  border: 1px solid var(--color-border);
  border-radius: 6px;
  padding: 8px 10px;
  overflow-x: auto;
  margin: 0.5em 0;
  font-size: 0.85em;
}

.chat-markdown :deep(code) {
  font-family: ui-monospace, 'SF Mono', Menlo, Monaco, monospace;
  font-size: 0.9em;
}

.chat-markdown :deep(:not(pre) > code) {
  background: color-mix(in oklch, var(--color-muted) 50%, transparent);
  padding: 0.15em 0.35em;
  border-radius: 4px;
}

.chat-markdown :deep(blockquote) {
  border-left: 3px solid var(--color-primary);
  margin: 0.5em 0;
  padding-left: 0.8em;
  color: var(--color-muted-foreground);
}

.chat-markdown :deep(strong) {
  font-weight: 600;
}

.user-message {
  max-width: 85%;
  border-radius: 14px 14px 4px 14px;
  background: color-mix(in oklch, var(--color-primary) 15%, transparent);
  border: 1px solid color-mix(in oklch, var(--color-primary) 25%, transparent);
  color: var(--color-foreground);
  padding: 8px 12px;
  font-size: var(--font-text-body);
  line-height: 1.5;
}

.assistant-message {
  max-width: 92%;
  border-radius: 14px 14px 14px 4px;
  background: color-mix(in oklch, var(--color-muted) 60%, transparent);
  border: 1px solid var(--color-border);
  padding: 8px 12px;
  font-size: var(--font-text-body);
  line-height: 1.6;
  color: var(--color-foreground);
}

/* streaming animation - blinking cursor */
.assistant-message.streaming .chat-markdown::after {
  content: '▋';
  display: inline;
  animation: cursor-blink 0.7s steps(1) infinite;
  color: var(--color-primary);
  font-weight: 300;
  margin-left: 1px;
}

@keyframes cursor-blink {
  0%, 50% { opacity: 1; }
  51%, 100% { opacity: 0; }
}

.timing-badge {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: var(--font-text-small);
  color: color-mix(in oklch, var(--color-muted-foreground) 70%, transparent);
  padding-left: 4px;
}

/* typing indicator */
.typing-indicator {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 12px 16px;
  background: color-mix(in oklch, var(--color-card) 70%, transparent);
  border: 1px solid color-mix(in oklch, var(--color-border) 30%, transparent);
  border-radius: 16px 16px 16px 4px;
}

.typing-indicator .dot {
  width: 6px;
  height: 6px;
  background: var(--color-primary);
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

.message-appear {
  animation: messageSlideIn 0.25s ease-out forwards;
}

@keyframes messageSlideIn {
  from {
    opacity: 0;
    transform: translateY(8px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

.empty-state {
  animation: fadeIn 0.3s ease-out;
}

@keyframes fadeIn {
  from { opacity: 0; }
  to { opacity: 1; }
}

.indexing-indicator {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 14px;
  background: color-mix(in oklch, var(--color-primary) 8%, transparent);
  border: 1px solid color-mix(in oklch, var(--color-primary) 20%, transparent);
  border-radius: 14px 14px 14px 4px;
}
</style>
