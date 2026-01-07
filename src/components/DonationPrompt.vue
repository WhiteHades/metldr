<script setup lang="ts">
import { onMounted } from 'vue'
import { Coffee, X, Heart } from 'lucide-vue-next'
import { useDonation } from '@/composables/useDonation'
import { APP_CONFIG } from '@/config/constants'

const { load, shouldShowPrompt, timeSaved, totalSummaries, dismissPrompt, markDonated } = useDonation()

function openDonation() {
  markDonated()
  window.open(APP_CONFIG.buyMeACoffee, '_blank')
}

onMounted(load)
</script>

<template>
  <Transition name="fade">
    <div v-if="shouldShowPrompt" class="donation-prompt">
      <button @click="dismissPrompt" class="dismiss-btn" aria-label="dismiss">
        <X :size="12" />
      </button>

      <div class="prompt-content">
        <div class="icon-wrap">
          <Heart :size="16" class="heart-icon" />
        </div>

        <div class="text">
          <p class="headline">
            you've saved <strong>{{ timeSaved }}</strong> with metldr!
          </p>
          <p class="sub">
            {{ totalSummaries }} summaries and counting. consider supporting development?
          </p>
        </div>

        <button @click="openDonation" class="donate-btn">
          <Coffee :size="12" />
          <span>buy me a coffee</span>
        </button>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.donation-prompt {
  position: relative;
  padding: 12px 14px;
  background: linear-gradient(135deg, 
    color-mix(in oklch, var(--color-amber-500, #f59e0b) 10%, var(--color-card)),
    var(--color-card)
  );
  border: 1px solid color-mix(in oklch, var(--color-amber-500, #f59e0b) 25%, var(--color-border));
  border-radius: 12px;
  margin: 8px 0;
}

.dismiss-btn {
  position: absolute;
  top: 6px;
  right: 6px;
  padding: 4px;
  border: none;
  background: transparent;
  color: var(--color-muted-foreground);
  cursor: pointer;
  border-radius: 4px;
  transition: all 0.15s ease;
}

.dismiss-btn:hover {
  background: var(--color-muted);
  color: var(--color-foreground);
}

.prompt-content {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.icon-wrap {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  background: color-mix(in oklch, var(--color-amber-500, #f59e0b) 20%, transparent);
  border-radius: 8px;
}

.heart-icon {
  color: #f59e0b;
  animation: pulse 2s ease-in-out infinite;
}

@keyframes pulse {
  0%, 100% { transform: scale(1); }
  50% { transform: scale(1.1); }
}

.text {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.headline {
  font-size: 12px;
  color: var(--color-foreground);
  margin: 0;
}

.headline strong {
  color: #f59e0b;
}

.sub {
  font-size: 10px;
  color: var(--color-muted-foreground);
  margin: 0;
}

.donate-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 8px 14px;
  background: #f59e0b;
  color: #000;
  border: none;
  border-radius: 8px;
  font-size: 11px;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.15s ease;
}

.donate-btn:hover {
  background: #fbbf24;
  transform: translateY(-1px);
}

/* transition */
.fade-enter-active, .fade-leave-active {
  transition: all 0.3s ease;
}

.fade-enter-from, .fade-leave-to {
  opacity: 0;
  transform: translateY(-8px);
}
</style>
