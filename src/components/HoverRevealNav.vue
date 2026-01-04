<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { FileText, BarChart3, Settings, ChevronDown } from 'lucide-vue-next'

interface Tab {
  key: string
  icon: typeof FileText
  label: string
}

interface Props {
  activeTab: string
  tabs?: Tab[]
}

withDefaults(defineProps<Props>(), {
  tabs: () => [
    { key: 'summary', icon: FileText, label: 'Summary' },
    { key: 'stats', icon: BarChart3, label: 'Stats' },
    { key: 'settings', icon: Settings, label: 'Settings' }
  ]
})

const emit = defineEmits<{
  'switch': [tab: string]
  'open': []
  'close': []
}>()

const isOpen = ref(false)
const hasSeenHint = ref(false)
let closeTimer: ReturnType<typeof setTimeout> | null = null

function open() {
  if (closeTimer) {
    clearTimeout(closeTimer)
    closeTimer = null
  }
  isOpen.value = true
  hasSeenHint.value = true
  emit('open')
}

function close() {
  isOpen.value = false
  closeTimer = setTimeout(() => {
    emit('close')
    closeTimer = null
  }, 120)
}

onMounted(() => {
  setTimeout(() => {
    if (!hasSeenHint.value) {
      open()
      setTimeout(() => {
        if (!isOpen.value) return
        close()
      }, 1800)
    }
  }, 800)
})
</script>

<template>
  <div 
    class="nav-wrap"
    @mouseenter="open"
    @mouseleave="close"
  >
    <div class="indicator">
      <div class="glow-line" />
      <ChevronDown 
        v-if="!isOpen" 
        :size="10" 
        class="hint-icon" 
      />
    </div>
    
    <Transition name="fade">
      <div v-if="isOpen" class="nav-bar">
        <button
          v-for="tab in tabs"
          :key="tab.key"
          @click="emit('switch', tab.key)"
          class="tab-btn"
          :class="{ active: activeTab === tab.key }"
        >
          <component :is="tab.icon" :size="12" :stroke-width="2" />
          <span>{{ tab.label }}</span>
        </button>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.nav-wrap {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  height: 20px;
  z-index: 100;
}

.indicator {
  position: absolute;
  top: 0;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  flex-direction: column;
  align-items: center;
}

.glow-line {
  width: 60px;
  height: 2px;
  background: linear-gradient(90deg, transparent, hsl(var(--primary) / 0.5), transparent);
  border-radius: 0 0 2px 2px;
  animation: glow 2.5s ease-in-out infinite;
}

.hint-icon {
  color: hsl(var(--muted-foreground) / 0.4);
  margin-top: 2px;
  animation: bob 2s ease-in-out infinite;
}

@keyframes glow {
  0%, 100% { 
    opacity: 0.3;
    box-shadow: 0 0 4px hsl(var(--primary) / 0.2);
  }
  50% { 
    opacity: 0.7;
    box-shadow: 0 0 8px hsl(var(--primary) / 0.4);
  }
}

@keyframes bob {
  0%, 100% { transform: translateY(0); opacity: 0.3; }
  50% { transform: translateY(2px); opacity: 0.5; }
}

.nav-bar {
  display: flex;
  justify-content: center;
  gap: 6px;
  padding: 8px 12px;
  background: hsl(var(--background));
  border-bottom: 1px solid hsl(var(--border));
  box-shadow: 0 2px 8px hsl(var(--foreground) / 0.1);
}

.tab-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  border-radius: 6px;
  font-size: 11px;
  font-weight: 500;
  color: hsl(var(--muted-foreground));
  background: hsl(var(--muted) / 0.3);
  border: 1px solid hsl(var(--border) / 0.5);
  cursor: pointer;
  transition: all 150ms ease;
}

.tab-btn:hover {
  color: hsl(var(--foreground));
  background: hsl(var(--muted));
  border-color: hsl(var(--primary) / 0.5);
  transform: translateY(-1px);
  box-shadow: 0 2px 8px hsl(var(--primary) / 0.15);
}

.tab-btn.active {
  color: hsl(var(--primary-foreground));
  background: hsl(var(--primary));
  border-color: hsl(var(--primary));
}

.fade-enter-active {
  transition: opacity 150ms ease, transform 150ms ease;
}

.fade-leave-active {
  transition: opacity 100ms ease, transform 100ms ease;
}

.fade-enter-from {
  opacity: 0;
  transform: translateY(-8px);
}

.fade-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}
</style>
