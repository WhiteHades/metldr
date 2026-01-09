<script setup lang="ts">
import { onMounted, computed, ref } from 'vue'
import { Coffee, X, Heart, Sparkles, Shield, Zap } from 'lucide-vue-next'
import { useDonation } from '@/composables/useDonation'
import { APP_CONFIG } from '@/config/constants'

const { load, shouldShowPrompt, moneySaved, dismissPrompt, markDonated } = useDonation()

// varied messaging for chat prompts
const MESSAGES = [
  {
    icon: Heart,
    headline: 'enjoying metldr?',
    text: 'built by a solo dev who just graduated. if this saves you time, a coffee would mean the world!',
    size: 'normal'
  },
  {
    icon: Sparkles,
    headline: 'local ai, zero cost',
    text: `you've saved $\${'{savings}'} vs cloud api's. exciting features coming, support if you can!`,
    size: 'compact'
  },
  {
    icon: Shield,
    headline: 'privacy-first ai',
    text: 'no data leaves your device. free and private. more features coming - support a dev who just started!',
    size: 'normal'
  },
  {
    icon: Zap,
    headline: 'powered by you',
    text: 'open source, no tracking, no api costs. supporting means helping a solo dev keep building.',
    size: 'wide'
  },
  {
    icon: Heart,
    headline: 'quick note',
    text: 'metldr is free. if you find it useful, a small donation helps a solo dev keep adding features.',
    size: 'compact'
  }
]

const messageIndex = ref(Math.floor(Math.random() * MESSAGES.length))
const currentMessage = computed(() => {
  const msg = MESSAGES[messageIndex.value]
  return {
    ...msg,
    text: msg.text.replace('{savings}', moneySaved.value)
  }
})

function handleDismiss() {
  dismissPrompt()
}

function openDonation() {
  markDonated()
  window.open(APP_CONFIG.buyMeACoffee, '_blank')
}

onMounted(load)
</script>

<template>
  <Transition name="fade">
    <div v-if="shouldShowPrompt" class="chat-donation" :class="currentMessage.size">
      <button @click="handleDismiss" class="dismiss-btn" aria-label="dismiss">
        <X :size="12" />
      </button>

      <div class="prompt-body">
        <component :is="currentMessage.icon" :size="14" class="prompt-icon" />
        <div class="prompt-text">
          <span class="headline">{{ currentMessage.headline }}</span>
          <span class="subtext">{{ currentMessage.text }}</span>
        </div>
      </div>

      <button @click="openDonation" class="coffee-btn">
        <Coffee :size="11" />
        <span>support</span>
      </button>
    </div>
  </Transition>
</template>

<style scoped>
.chat-donation {
  position: relative;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  background: linear-gradient(135deg,
    color-mix(in oklch, #f59e0b 10%, var(--color-card)),
    var(--color-card)
  );
  border: 1px solid color-mix(in oklch, #f59e0b 30%, var(--color-border));
  border-radius: 10px;
  margin: 8px 0;
}

.chat-donation.compact {
  padding: 8px 10px;
  gap: 8px;
}

.chat-donation.wide {
  flex-wrap: wrap;
  padding: 12px 14px;
}

.dismiss-btn {
  position: absolute;
  top: 6px;
  right: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  background: none;
  border: none;
  color: var(--color-muted-foreground);
  cursor: pointer;
  border-radius: 4px;
  transition: all 0.15s ease;
}

.dismiss-btn:hover {
  background: var(--color-muted);
  color: var(--color-foreground);
}

.prompt-body {
  flex: 1;
  display: flex;
  align-items: flex-start;
  gap: 8px;
  min-width: 0;
}

.prompt-icon {
  color: #f59e0b;
  flex-shrink: 0;
  margin-top: 2px;
}

.prompt-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.headline {
  font-size: 11px;
  font-weight: 600;
  color: var(--color-foreground);
}

.subtext {
  font-size: 10px;
  color: var(--color-muted-foreground);
  line-height: 1.3;
}

.coffee-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 6px 10px;
  background: #f59e0b;
  color: #000;
  border: none;
  border-radius: 6px;
  font-size: 10px;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.15s ease;
  white-space: nowrap;
  flex-shrink: 0;
}

.coffee-btn:hover {
  background: #fbbf24;
  transform: translateY(-1px);
}

/* transitions */
.fade-enter-active, .fade-leave-active {
  transition: all 0.3s ease;
}

.fade-enter-from, .fade-leave-to {
  opacity: 0;
  transform: translateY(-8px);
}
</style>
