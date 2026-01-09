<script setup lang="ts">
import { ref, computed, nextTick, watch, onMounted, onUnmounted } from 'vue'
import { formatTime, stripThinking } from '@/utils/text'
import { marked } from 'marked'
import { MessageSquare, Loader2 } from 'lucide-vue-next'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui'
import ChatComposer from '@/components/ChatComposer.vue'
import { useIndexingProgress } from '@/composables/useIndexingProgress'
import type { AppChatMessage } from '@/types'

interface Props {
  chatMessages: AppChatMessage[]
  chatLoading: boolean
  chatIndexing?: boolean
  summaryLoading?: boolean
  chatDisabled: boolean
  disabledReason?: string
  isViewingEmailThread: boolean
  currentUrl?: string | null
}

const props = defineProps<Props>()
const chatInput = defineModel<string>('chatInput', { required: true })

const emit = defineEmits<{
  'send': []
  'clear': []
}>()

const viewportRef = ref<HTMLDivElement | null>(null)
const composerRef = ref<InstanceType<typeof ChatComposer> | null>(null)

const isRunning = computed(() => props.chatLoading)
const isThreadEmpty = computed(() => props.chatMessages.length === 0 && !props.chatLoading)

const isIndexingOrProcessing = computed(() => props.chatIndexing || props.summaryLoading)
const inputDisabled = computed(() => props.chatDisabled || isIndexingOrProcessing.value)
const inputPlaceholder = computed(() => {
  if (props.summaryLoading) return 'analyzing page, please wait...'
  if (props.chatIndexing) return 'indexing document...'
  return 'ask anything about this page...'
})

// url-scoped indexing progress
const { isIndexing, indexingProgress, indexingMessage, setupListener, cleanupListener } = 
  useIndexingProgress(() => props.currentUrl || '')

onMounted(() => {
  setupListener()
})

onUnmounted(() => {
  cleanupListener()
})

function focusInput() {
  composerRef.value?.focus()
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

watch(() => props.chatMessages.length, () => scrollToBottom(true))
watch(() => props.chatLoading, () => scrollToBottom())

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
  if (!isRunning.value && !isIndexingOrProcessing.value) {
    emit('send')
  }
}

function handleClear() {
  emit('clear')
}

defineExpose({
  viewportRef,
  composerRef,
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
        <ChatComposer
          ref="composerRef"
          v-model="chatInput"
          placeholder="ask anything about this page..."
          :disabled="chatDisabled"
          :loading="isRunning"
          :show-clear="chatMessages.length > 0"
          @send="handleSend"
          @clear="handleClear"
        />
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
              <div v-if="msg.timing">
                <TooltipProvider>
                  <Tooltip>
                    <TooltipTrigger as-child>
                      <div class="timing-badge cursor-help">
                        <span>{{ formatTime(msg.timing.total) }}</span>
                        <span v-if="msg.timing.model">· {{ msg.timing.model }}</span>
                      </div>
                    </TooltipTrigger>
                    <TooltipContent class="min-w-40 p-2.5">
                      <div class="text-xs font-medium text-foreground mb-1.5 uppercase tracking-wide">time breakdown</div>
                      <div class="space-y-1 text-xs">
                        <div v-if="msg.timing.rag" class="flex justify-between gap-3">
                          <span class="text-foreground/70">processing</span>
                          <span class="text-foreground font-mono">{{ formatTime(msg.timing.rag) }}</span>
                        </div>
                        <div class="flex justify-between gap-3">
                          <span class="text-foreground/70">ai</span>
                          <span class="text-foreground font-mono">{{ formatTime(msg.timing.llm || msg.timing.total) }}</span>
                        </div>
                        <div class="flex justify-between gap-3 pt-1 border-t border-zinc-800">
                          <span class="text-foreground">total</span>
                          <span class="text-primary font-mono font-medium">{{ formatTime(msg.timing.total) }}</span>
                        </div>
                        <div v-if="msg.timing.model" class="pt-1 text-foreground/60 truncate">
                          {{ msg.timing.model }}
                        </div>
                      </div>
                    </TooltipContent>
                  </Tooltip>
                </TooltipProvider>
              </div>
            </div>
          </template>
          
          <!-- indexing indicator with inline progress -->
          <div v-if="chatIndexing || isIndexing" class="flex items-start gap-2">
            <div class="indexing-indicator">
              <Loader2 :size="14" class="animate-spin text-primary" />
              <span class="text-xs text-muted-foreground">
                {{ indexingMessage || 'indexing...' }}
                <span v-if="indexingProgress > 0" class="text-primary font-medium">{{ indexingProgress }}%</span>
              </span>
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
        <ChatComposer
          ref="composerRef"
          v-model="chatInput"
          :placeholder="inputPlaceholder"
          :disabled="inputDisabled"
          :loading="isRunning"
          :show-clear="chatMessages.length > 0"
          @send="handleSend"
          @clear="handleClear"
        />
      </div>
    </template>
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
  word-break: break-all;
  overflow-wrap: anywhere;
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
  word-break: break-word;
  overflow-wrap: anywhere;
}

/* streaming animation - blinking cursor at end of last content */
.assistant-message.streaming .chat-markdown :deep(> *:last-child)::after {
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
