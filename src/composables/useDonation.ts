// donation prompt state management
import { ref, computed } from 'vue'
import { storageService } from '@/services/StorageService'
import { analyticsService } from '@/services/AnalyticsService'

interface DonationState {
  lastPromptShown: number  // timestamp
  dismissCount: number
  donated: boolean
}

const DEFAULT_STATE: DonationState = {
  lastPromptShown: 0,
  dismissCount: 0,
  donated: false
}

// thresholds
const MIN_SUMMARIES = 10
const DAYS_BETWEEN_PROMPTS = 14
const MAX_DISMISSES = 3

const state = ref<DonationState>(DEFAULT_STATE)
const totalSummaries = ref(0)
const loaded = ref(false)

export function useDonation() {
  async function load(): Promise<void> {
    if (loaded.value) return

    try {
      const [stored, analytics] = await Promise.all([
        storageService.get<DonationState>('donationState', DEFAULT_STATE),
        analyticsService.getAnalyticsSummary(30)
      ])
      state.value = stored
      totalSummaries.value = analytics.totalSummaries
      loaded.value = true
    } catch {
      loaded.value = true
    }
  }

  const shouldShowPrompt = computed(() => {
    // never show if user donated or dismissed too many times
    if (state.value.donated || state.value.dismissCount >= MAX_DISMISSES) return false

    // need minimum summaries
    if (totalSummaries.value < MIN_SUMMARIES) return false

    // rate limit: once per 14 days
    const daysSince = (Date.now() - state.value.lastPromptShown) / (1000 * 60 * 60 * 24)
    return daysSince >= DAYS_BETWEEN_PROMPTS
  })

  const timeSaved = computed(() => {
    // estimate 5 min saved per summary
    const minutes = totalSummaries.value * 5
    if (minutes >= 60) return `${Math.round(minutes / 60)} hours`
    return `${minutes} minutes`
  })

  async function dismissPrompt(): Promise<void> {
    state.value = {
      ...state.value,
      dismissCount: state.value.dismissCount + 1,
      lastPromptShown: Date.now()
    }
    await storageService.set('donationState', state.value)
  }

  async function markDonated(): Promise<void> {
    state.value = {
      ...state.value,
      donated: true,
      lastPromptShown: Date.now()
    }
    await storageService.set('donationState', state.value)
  }

  return {
    load,
    shouldShowPrompt,
    timeSaved,
    totalSummaries,
    dismissPrompt,
    markDonated
  }
}
