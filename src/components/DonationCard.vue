<script setup lang="ts">
import { computed } from 'vue'
import { Heart, Coffee } from 'lucide-vue-next'
import { APP_CONFIG } from '@/config/constants'

defineProps<{
  savings?: number  // optional, if provided shows savings
}>()

const donationUrl = APP_CONFIG.buyMeACoffee
</script>

<template>
  <div class="donation-card">
    <div class="card-header">
      <div class="icon-wrap">
        <Heart :size="12" class="heart-icon" />
      </div>
      <span class="title">support metldr</span>
    </div>
    
    <p class="description">
      <template v-if="savings && savings > 0">
        you've saved <strong class="savings">${{ savings.toFixed(2) }}</strong> using local ai.
      </template>
      <template v-else>
        if metldr saves you time, consider supporting development for future features.
      </template>
    </p>
    
    <a :href="donationUrl" target="_blank" class="donate-btn">
      <Coffee :size="12" />
      <span>buy me a coffee</span>
    </a>
  </div>
</template>

<style scoped>
.donation-card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px;
  background: linear-gradient(135deg, 
    color-mix(in oklch, #f59e0b 8%, var(--color-card)),
    var(--color-card)
  );
  border: 1px solid color-mix(in oklch, #f59e0b 25%, var(--color-border));
  border-radius: 0.75rem;
}

.card-header {
  display: flex;
  align-items: center;
  gap: 8px;
}

.icon-wrap {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  background: color-mix(in oklch, #f59e0b 20%, transparent);
  border-radius: 6px;
}

.heart-icon {
  color: #f59e0b;
  animation: pulse 2s ease-in-out infinite;
}

@keyframes pulse {
  0%, 100% { transform: scale(1); }
  50% { transform: scale(1.1); }
}

.title {
  font-size: var(--font-text-body);
  font-weight: 600;
  color: var(--color-foreground);
}

.description {
  font-size: var(--font-text-small);
  color: var(--color-muted-foreground);
  line-height: 1.4;
  margin: 0;
}

.savings {
  color: #34d399;
  font-weight: 700;
}

.donate-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 8px 14px;
  background: #f59e0b;
  color: #000;
  border: none;
  border-radius: 8px;
  font-size: 11px;
  font-weight: 600;
  text-decoration: none;
  cursor: pointer;
  transition: all 0.15s ease;
  width: fit-content;
}

.donate-btn:hover {
  background: #fbbf24;
  transform: translateY(-1px);
}
</style>
