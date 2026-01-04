<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed } from 'vue'
import { 
  FileText, Clock, Loader2, ChevronRight, ChevronUp, Trash2, 
  MessageSquare, Globe, Mail, Flame, BookOpen,
  Cpu, History, TrendingUp, BarChart3, Zap, Timer, Reply, Sparkles
} from 'lucide-vue-next'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui'
import { statsService, type UsageStats, type ActivityItem } from '@/services/StatsService'
import { analyticsService, type AnalyticsSummary } from '@/services/AnalyticsService'
import { cacheService } from '@/services/CacheService'
import { logger } from '@/services/LoggerService'

const log = logger.createScoped('HistoryManager')
const POLL_INTERVAL_MS = 4000

const props = defineProps<{ limit?: number }>()

const activities = ref<ActivityItem[]>([])
const stats = ref<UsageStats>({
  totalEmails: 0, totalPages: 0, totalInteractions: 0,
  estimatedTimeSaved: 0, thisWeek: 0, today: 0,
  totalChatMessages: 0, avgResponseTime: 0
})
const analytics = ref<AnalyticsSummary | null>(null)
const loading = ref<boolean>(true)
const showRecentActivity = ref(true)
let messageListener: ((message: { type: string }) => void) | null = null
let pollIntervalId: ReturnType<typeof setInterval> | null = null

async function loadData(): Promise<void> {
  try {
    const [statsData, activityData, analyticsData] = await Promise.all([
      statsService.getStats(),
      statsService.getRecentActivity(props.limit ?? 10),
      analyticsService.getAnalyticsSummary(30)
    ])
    stats.value = statsData
    activities.value = activityData
    analytics.value = analyticsData
  } catch (error) {
    log.error('failed to load stats:', error)
  } finally {
    loading.value = false
  }
}

async function clearHistory(): Promise<void> {
  if (!confirm('clear all activity history?')) return
  try {
    await Promise.all([cacheService.clearAll(), analyticsService.clearAllData()])
    activities.value = []
    analytics.value = null
  } catch (err) {
    log.error('failed to clear history:', err)
  }
}

function formatTimestamp(ts: number): string {
  const date = new Date(ts)
  const now = new Date()
  const diffMins = Math.floor((now.getTime() - date.getTime()) / 60000)
  if (diffMins < 1) return 'just now'
  if (diffMins < 60) return `${diffMins}m ago`
  const diffHours = Math.floor(diffMins / 60)
  if (diffHours < 24) return `${diffHours}h ago`
  const diffDays = Math.floor(diffHours / 24)
  if (diffDays < 7) return `${diffDays}d ago`
  return date.toLocaleDateString()
}

function openActivity(item: ActivityItem): void {
  if (item.type === 'email') {
    chrome.tabs.create({ url: `https://mail.google.com/mail/u/0/#inbox/${item.id}` })
  } else {
    chrome.tabs.create({ url: item.id })
  }
}

// formatters
const fmtTime = (s: number) => {
  if (!s || s < 60) return '<1m'
  const m = Math.round(s / 60)
  if (m < 60) return `${m}m`
  const h = m / 60
  return h < 24 ? `${h.toFixed(1)}h` : `${(h / 24).toFixed(1)}d`
}
const fmtNum = (n: number) => {
  if (!n) return '0'
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)}k`
  return String(Math.round(n))
}
const fmtMs = (ms: number) => !ms ? '—' : ms < 1000 ? `${Math.round(ms)}ms` : `${(ms / 1000).toFixed(1)}s`
const fmtHour = (h: number) => h === 0 ? '12am' : h < 12 ? `${h}am` : h === 12 ? '12pm' : `${h - 12}pm`

// computed
const hasData = computed(() => analytics.value && (analytics.value.totalSummaries > 0 || analytics.value.totalChatMessages > 0))

// all stats
const summaryCount = computed(() => analytics.value?.totalSummaries || 0)
const chatCount = computed(() => analytics.value?.totalChatMessages || 0)
const timeSavedSec = computed(() => analytics.value?.readingTimeSaved || 0)
const streak = computed(() => analytics.value?.currentStreak || 0)
const longestStreak = computed(() => analytics.value?.longestStreak || 0)
const emailsProcessed = computed(() => analytics.value?.totalEmailsProcessed || 0)
const pagesProcessed = computed(() => analytics.value?.totalPagesProcessed || 0)
const pdfsProcessed = computed(() => analytics.value?.totalPdfsProcessed || 0)
const wordsProcessed = computed(() => analytics.value?.totalWordsProcessed || 0)
const wordsLookedUp = computed(() => analytics.value?.totalWordsLookedUp || 0)
const tokensIn = computed(() => analytics.value?.totalTokensInput || 0)
const tokensOut = computed(() => analytics.value?.totalTokensOutput || 0)
const avgResponse = computed(() => analytics.value?.avgResponseTime || 0)
const fastestResponse = computed(() => analytics.value?.fastestResponse || 0)
const panelTime = computed(() => analytics.value?.totalTimeInPanel || 0)
const avgSession = computed(() => analytics.value?.avgSessionDuration || 0)
const peakHour = computed(() => analytics.value?.peakHour ?? -1)
const peakDay = computed(() => analytics.value?.peakDay || '')

// hourly pattern
const hourlyPattern = computed(() => analytics.value?.hourlyPatterns || [])
const maxHourly = computed(() => Math.max(...hourlyPattern.value, 1))

// ratios for bars
const totalContent = computed(() => emailsProcessed.value + pagesProcessed.value + pdfsProcessed.value)
const emailRatio = computed(() => totalContent.value > 0 ? (emailsProcessed.value / totalContent.value) * 100 : 50)

// new insights
const topModel = computed(() => {
  const models = analytics.value?.modelBreakdown || []
  return models.length ? models[0].model : 'gemini-nano'
})

const avgDocSize = computed(() => analytics.value?.avgContentLength || 0)

const contentBreakdown = computed(() => {
  const total = totalContent.value || 1
  return [
    { 
      label: 'Emails', 
      count: emailsProcessed.value, 
      ratio: (emailsProcessed.value / total) * 100,
      color: 'c1',
      icon: Mail
    },
    { 
      label: 'Pages', 
      count: pagesProcessed.value, 
      ratio: (pagesProcessed.value / total) * 100,
      color: 'c3',
      icon: Globe
    },
    { 
      label: 'PDFs', 
      count: pdfsProcessed.value, 
      ratio: (pdfsProcessed.value / total) * 100,
      color: 'c5',
      icon: FileText
    }
  ].filter(c => c.count > 0)
})


onMounted(async () => {
  await loadData()
  pollIntervalId = setInterval(loadData, POLL_INTERVAL_MS)
  messageListener = (msg) => {
    if (msg.type === 'SUMMARY_ADDED' || msg.type === 'SESSION_UPDATED' || msg.type === 'STATS_UPDATED') loadData()
  }
  chrome.runtime.onMessage.addListener(messageListener)
})

onUnmounted(() => {
  if (pollIntervalId) clearInterval(pollIntervalId)
  if (messageListener) chrome.runtime.onMessage.removeListener(messageListener)
})

defineExpose({ refresh: loadData, stats, analytics })
</script>

<template>
  <div class="stats-container">
    <div v-if="loading" class="flex items-center justify-center py-8">
      <Loader2 class="w-5 h-5 animate-spin text-primary" :stroke-width="2" />
    </div>

    <template v-else-if="hasData && analytics">
      <!-- hero stats - 3x2 compact grid -->
      <div class="hero-grid">
        <TooltipProvider>
          <Tooltip>
            <TooltipTrigger as-child>
              <div class="hero-card">
                <FileText :size="14" class="hero-icon violet" />
                <div class="hero-data">
                  <span class="hero-value">{{ fmtNum(summaryCount) }}</span>
                  <span class="hero-label">summaries</span>
                </div>
              </div>
            </TooltipTrigger>
            <TooltipContent>AI summaries of emails & pages</TooltipContent>
          </Tooltip>
        </TooltipProvider>

        <TooltipProvider>
          <Tooltip>
            <TooltipTrigger as-child>
              <div class="hero-card">
                <MessageSquare :size="14" class="hero-icon blue" />
                <div class="hero-data">
                  <span class="hero-value">{{ fmtNum(chatCount) }}</span>
                  <span class="hero-label">messages</span>
                </div>
              </div>
            </TooltipTrigger>
            <TooltipContent>Chat messages with AI</TooltipContent>
          </Tooltip>
        </TooltipProvider>

        <TooltipProvider>
          <Tooltip>
            <TooltipTrigger as-child>
              <div class="hero-card">
                <Clock :size="14" class="hero-icon emerald" />
                <div class="hero-data">
                  <span class="hero-value">{{ fmtTime(timeSavedSec) }}</span>
                  <span class="hero-label">saved</span>
                </div>
              </div>
            </TooltipTrigger>
            <TooltipContent>Estimated reading time saved</TooltipContent>
          </Tooltip>
        </TooltipProvider>

        <TooltipProvider>
          <Tooltip>
            <TooltipTrigger as-child>
              <div class="hero-card">
                <Flame :size="14" class="hero-icon orange" />
                <div class="hero-data">
                  <span class="hero-value">{{ streak }}</span>
                  <span class="hero-label">streak</span>
                </div>
              </div>
            </TooltipTrigger>
            <TooltipContent>{{ streak }} consecutive days (best: {{ longestStreak }})</TooltipContent>
          </Tooltip>
        </TooltipProvider>

        <TooltipProvider>
          <Tooltip>
            <TooltipTrigger as-child>
              <div class="hero-card">
                <BookOpen :size="14" class="hero-icon amber" />
                <div class="hero-data">
                  <span class="hero-value">{{ fmtNum(wordsProcessed) }}</span>
                  <span class="hero-label">words</span>
                </div>
              </div>
            </TooltipTrigger>
            <TooltipContent>Total words processed</TooltipContent>
          </Tooltip>
        </TooltipProvider>

        <TooltipProvider>
          <Tooltip>
            <TooltipTrigger as-child>
              <div class="hero-card">
                <TrendingUp :size="14" class="hero-icon cyan" />
                <div class="hero-data">
                  <span class="hero-value">{{ fmtNum(wordsLookedUp) }}</span>
                  <span class="hero-label">lookups</span>
                </div>
              </div>
            </TooltipTrigger>
            <TooltipContent>Dictionary definitions viewed</TooltipContent>
          </Tooltip>
        </TooltipProvider>
      </div>

      <div class="section-card">
        <div class="section-header">
          <BookOpen :size="11" class="text-amber-400" />
          <span>Content</span>
        </div>
        
        <div class="content-bar">
          <TooltipProvider v-for="(item, i) in contentBreakdown" :key="i">
            <Tooltip>
              <TooltipTrigger as-child>
                <div class="bar-seg" :class="item.color" :style="{ width: Math.max(item.ratio, 15) + '%' }">
                  <component :is="item.icon" :size="10" />
                  <span>{{ fmtNum(item.count) }}</span>
                </div>
              </TooltipTrigger>
              <TooltipContent>{{ item.count }} {{ item.label.toLowerCase() }} summarized</TooltipContent>
            </Tooltip>
          </TooltipProvider>
        </div>

        <div class="content-footer">
          <div class="flex gap-3">
            <span v-for="(item, i) in contentBreakdown" :key="i" class="legend">
              <span class="dot" :class="item.color" /> {{ item.label }}
            </span>
          </div>
          <div class="flex items-center gap-1.5 text-foreground/50 text-[10px]">
             <span>avg doc: {{ fmtNum(avgDocSize) }} words</span>
          </div>
        </div>
      </div>

      <div class="section-card">
        <div class="section-header">
          <Cpu :size="11" class="text-primary" />
          <span>AI</span>
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger as-child>
                <span class="section-badge">{{ fmtNum(tokensIn + tokensOut) }} tokens</span>
              </TooltipTrigger>
              <TooltipContent>{{ fmtNum(tokensIn) }} input / {{ fmtNum(tokensOut) }} output</TooltipContent>
            </Tooltip>
          </TooltipProvider>
        </div>
        
        <div class="perf-row">
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger as-child>
                <div class="perf-item">
                  <div class="perf-bar-wrap">
                    <div class="perf-bar violet" :style="{ width: Math.min(avgResponse / 20, 100) + '%' }" />
                  </div>
                  <span class="perf-val">{{ fmtMs(avgResponse) }}</span>
                  <span class="perf-lbl">avg</span>
                </div>
              </TooltipTrigger>
              <TooltipContent>average response time</TooltipContent>
            </Tooltip>
          </TooltipProvider>

          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger as-child>
                <div class="perf-item">
                  <div class="perf-bar-wrap">
                    <div class="perf-bar emerald" :style="{ width: Math.min(fastestResponse / 10, 100) + '%' }" />
                  </div>
                  <span class="perf-val emerald">{{ fmtMs(fastestResponse) }}</span>
                  <span class="perf-lbl">fast</span>
                </div>
              </TooltipTrigger>
              <TooltipContent>fastest response recorded</TooltipContent>
            </Tooltip>
          </TooltipProvider>

          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger as-child>
                <div class="perf-item">
                  <div class="perf-bar-wrap">
                    <div class="perf-bar amber" style="width: 100%" />
                  </div>
                  <span class="perf-val amber">{{ topModel }}</span>
                  <span class="perf-lbl">model</span>
                </div>
              </TooltipTrigger>
              <TooltipContent>Most used model</TooltipContent>
            </Tooltip>
          </TooltipProvider>
        </div>
      </div>

      <!-- usage with mini chart -->
      <div class="section-card">
        <div class="section-header">
          <BarChart3 :size="11" class="text-emerald-400" />
          <span>Usage</span>
        </div>

        <div class="usage-row">
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger as-child>
                <div class="usage-item">
                  <Timer :size="11" class="text-muted-foreground" />
                  <span class="usage-val">{{ fmtTime(panelTime) }}</span>
                  <span class="usage-lbl">total</span>
                </div>
              </TooltipTrigger>
              <TooltipContent>Total panel time</TooltipContent>
            </Tooltip>
          </TooltipProvider>

          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger as-child>
                <div class="usage-item">
                  <Zap :size="11" class="text-muted-foreground" />
                  <span class="usage-val">{{ fmtTime(avgSession) }}</span>
                  <span class="usage-lbl">session</span>
                </div>
              </TooltipTrigger>
              <TooltipContent>Average session length</TooltipContent>
            </Tooltip>
          </TooltipProvider>
        </div>

        <!-- hourly mini chart with theme colors -->
        <div v-if="hourlyPattern.length" class="mini-chart">
          <div class="chart-bars">
            <TooltipProvider v-for="(val, i) in hourlyPattern" :key="i">
              <Tooltip>
                <TooltipTrigger as-child>
                  <div 
                    class="chart-bar"
                    :class="{ peak: i === peakHour }"
                    :style="{ height: Math.max((val / maxHourly * 100), 4) + '%' }"
                  />
                </TooltipTrigger>
                <TooltipContent>{{ fmtHour(i) }}: {{ Math.round(val) }}min</TooltipContent>
              </Tooltip>
            </TooltipProvider>
          </div>
          <div class="chart-labels">
            <span>12am</span>
            <span>6am</span>
            <span>12pm</span>
            <span>6pm</span>
          </div>
        </div>

        <div class="usage-footer">
          <span v-if="peakHour >= 0">Peak <strong>{{ fmtHour(peakHour) }}</strong></span>
          <span v-if="peakDay">Active <strong>{{ peakDay }}</strong></span>
        </div>
      </div>

      <!-- top sites -->
      <div v-if="analytics.topDomains?.length" class="section-card">
        <div class="section-header">
          <Globe :size="11" class="text-cyan-400" />
          <span>Top Sites</span>
        </div>
        <div class="sites-list">
          <div v-for="(site, i) in analytics.topDomains.slice(0, 3)" :key="site.domain" class="site-row">
            <span class="site-rank">{{ i + 1 }}</span>
            <span class="site-domain">{{ site.domain }}</span>
            <span class="site-count">{{ site.count }}</span>
          </div>
        </div>
      </div>
    </template>

    <!-- empty state -->
    <div v-else-if="!loading" class="empty-state">
      <FileText :size="24" class="text-muted-foreground/40" />
      <p>no activity yet</p>
      <p class="sub">start summarising to see stats</p>
    </div>

    <!-- recent activity -->
    <div class="activity-section">
      <button @click="showRecentActivity = !showRecentActivity" class="activity-header">
        <div class="header-left">
          <History :size="10" class="text-muted-foreground" />
          <span>Recent</span>
          <span v-if="activities.length" class="count">{{ activities.length }}</span>
        </div>
        <ChevronUp :size="11" :class="['chevron', { collapsed: !showRecentActivity }]" />
      </button>

      <Transition name="slide">
        <div v-if="showRecentActivity" class="activity-content">
          <div v-if="!activities.length" class="activity-empty">no recent activity</div>
          <div v-else class="activity-list">
            <div v-for="item in activities" :key="item.id" class="activity-item" @click="openActivity(item)">
              <div class="item-icon" :class="item.type">
                <Mail v-if="item.type === 'email'" :size="9" />
                <Globe v-else :size="9" />
              </div>
              <div class="item-content">
                <p class="item-title">{{ item.title }}</p>
                <div class="item-meta">
                  <span>{{ formatTimestamp(item.timestamp) }}</span>
                  <span v-if="item.hasSummary" class="tag violet">summary</span>
                  <span v-if="item.hasChat" class="tag cyan">{{ item.chatCount }}</span>
                </div>
              </div>
              <ChevronRight :size="10" class="item-arrow" />
            </div>
          </div>
          <button v-if="activities.length" @click="clearHistory" class="clear-btn">
            <Trash2 :size="9" /> clear
          </button>
        </div>
      </Transition>
    </div>
  </div>
</template>

<style scoped>
.stats-container {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

/* theme colors */
.violet { color: var(--color-primary); }
.blue { color: #60a5fa; }
.emerald { color: #34d399; }
.orange { color: #fb923c; }
.cyan { color: #22d3ee; }
.amber { color: #fbbf24; }
.pink { color: #f472b6; }

/* hero grid 3x2 */
.hero-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 6px;
}

.hero-card {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  background: var(--color-card);
  border: 1px solid var(--color-border);
  border-radius: 0.75rem;
  cursor: help;
  transition: all 0.15s ease;
}

.hero-card:hover {
  background: color-mix(in oklch, var(--color-muted) 70%, transparent);
}

.hero-icon {
  flex-shrink: 0;
}

.hero-data {
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.hero-value {
  font-family: 'JetBrains Mono', ui-monospace, monospace;
  font-size: var(--font-text-body);
  font-weight: 700;
  color: var(--color-foreground);
  line-height: 1;
}

.hero-label {
  font-size: var(--font-text-small);
  color: var(--color-muted-foreground);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* section cards */
.section-card {
  padding: 8px 10px;
  background: var(--color-card);
  border: 1px solid var(--color-border);
  border-radius: 0.75rem;
}

.section-header {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: var(--font-text-small);
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.03em;
  color: var(--color-muted-foreground);
  margin-bottom: 6px;
}

.section-value {
  margin-left: auto;
  font-family: 'JetBrains Mono', ui-monospace, monospace;
  font-size: var(--font-text-small);
  color: var(--color-foreground);
}

.section-badge {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 2px 6px;
  background: color-mix(in oklch, var(--color-primary) 15%, transparent);
  border-radius: 4px;
  font-family: 'JetBrains Mono', ui-monospace, monospace;
  font-size: var(--font-text-small);
  text-transform: lowercase;
  color: var(--color-primary);
  cursor: help;
}

.section-badge.small {
  padding: 1px 5px;
  font-size: 9px;
}

/* content bar - chat style */
.content-bar {
  display: flex;
  height: 32px;
  gap: 3px;
  padding: 3px;
  background: color-mix(in oklch, var(--color-card) 60%, transparent);
  border-radius: 12px;
  border: 1px solid color-mix(in oklch, var(--color-border) 40%, transparent);
}

.bar-seg {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  min-width: 40px;
  font-family: 'JetBrains Mono', ui-monospace, monospace;
  font-size: 10px;
  font-weight: 600;
  cursor: help;
  transition: filter 0.15s ease, transform 0.15s ease;
  --_bg: var(--color-chart-1);
  background: color-mix(in oklch, var(--_bg) 85%, var(--color-card));
  border: 1px solid color-mix(in oklch, var(--_bg) 30%, transparent);
  color: color-mix(in oklch, var(--_bg) 60%, black);
  border-radius: 8px;
}

.bar-seg:hover {
  filter: brightness(1.05);
  transform: scale(1.02);
}

.bar-seg.c1 { --_bg: var(--color-chart-1); }
.bar-seg.c2 { --_bg: var(--color-chart-2); }
.bar-seg.c3 { --_bg: var(--color-chart-3); }
.bar-seg.c4 { --_bg: var(--color-chart-4); }
.bar-seg.c5 { --_bg: var(--color-chart-5); }

.content-footer {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 5px;
  font-size: var(--font-text-small);
  color: var(--color-muted-foreground);
}

.legend {
  display: flex;
  align-items: center;
  gap: 3px;
}

.legend.pink {
  margin-left: auto;
  color: var(--color-chart-5);
  cursor: help;
}

.dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  --_bg: var(--color-chart-1);
  background: var(--_bg);
}

.dot.c1 { --_bg: var(--color-chart-1); }
.dot.c2 { --_bg: var(--color-chart-2); }
.dot.c3 { --_bg: var(--color-chart-3); }
.dot.c4 { --_bg: var(--color-chart-4); }
.dot.c5 { --_bg: var(--color-chart-5); }

/* perf row with mini bars */
.perf-row {
  display: flex;
  gap: 12px;
}

.perf-item {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
  cursor: help;
}

.perf-bar-wrap {
  height: 4px;
  background: var(--color-muted);
  border-radius: 2px;
  overflow: hidden;
}

.perf-bar {
  height: 100%;
  border-radius: 2px;
}

.perf-bar.violet { background: var(--color-primary); }
.perf-bar.emerald { background: #34d399; }

.perf-val {
  font-family: 'JetBrains Mono', ui-monospace, monospace;
  font-size: var(--font-text-body);
  font-weight: 600;
  color: var(--color-foreground);
}

.perf-val.emerald { color: #34d399; }

.perf-lbl {
  font-size: var(--font-text-small);
  color: var(--color-muted-foreground);
}

/* usage row */
.usage-row {
  display: flex;
  gap: 16px;
  margin-bottom: 6px;
}

.usage-item {
  display: flex;
  align-items: center;
  gap: 5px;
  cursor: help;
}

.usage-val {
  font-family: 'JetBrains Mono', ui-monospace, monospace;
  font-size: var(--font-text-body);
  font-weight: 600;
  color: var(--color-foreground);
}

.usage-lbl {
  font-size: var(--font-text-small);
  color: var(--color-muted-foreground);
}

/* mini chart */
.mini-chart {
  margin-top: 4px;
}

.chart-bars {
  display: flex;
  align-items: flex-end;
  height: 28px;
  gap: 1px;
}

.chart-bar {
  flex: 1;
  min-height: 2px;
  --_bg: var(--color-chart-1);
  background: color-mix(in oklch, var(--_bg) 50%, transparent);
  border-radius: 2px 2px 0 0;
  cursor: help;
  transition: background 0.15s ease;
}

.chart-bar.peak {
  background: var(--_bg);
}

.chart-bar:hover {
  background: var(--_bg);
}

.chart-labels {
  display: flex;
  justify-content: space-between;
  font-size: 7px;
  color: var(--color-muted-foreground);
  margin-top: 2px;
}

.usage-footer {
  display: flex;
  justify-content: space-between;
  font-size: var(--font-text-small);
  color: var(--color-muted-foreground);
  margin-top: 4px;
}

.usage-footer strong {
  color: var(--color-foreground);
}

/* insights with progress bars */
.insights-grid {
  display: flex;
  gap: 8px;
}

.insight-item {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  cursor: help;
}

.insight-val {
  font-family: 'JetBrains Mono', ui-monospace, monospace;
  font-size: var(--font-text-body);
  font-weight: 600;
  color: var(--color-foreground);
}

.insight-bar-wrap {
  width: 100%;
  height: 3px;
  background: var(--color-muted);
  border-radius: 2px;
  overflow: hidden;
}

.insight-bar {
  height: 100%;
  border-radius: 2px;
  --_bg: var(--color-chart-1);
  background: color-mix(in oklch, var(--_bg) 60%, transparent);
}

.insight-bar[style*="--color-idx: 1"] { --_bg: var(--color-chart-1); }
.insight-bar[style*="--color-idx: 2"] { --_bg: var(--color-chart-2); }
.insight-bar[style*="--color-idx: 3"] { --_bg: var(--color-chart-3); }
.insight-bar[style*="--color-idx: 4"] { --_bg: var(--color-chart-4); }
.insight-bar[style*="--color-idx: 5"] { --_bg: var(--color-chart-5); }

.insight-lbl {
  font-size: var(--font-text-small);
  color: var(--color-muted-foreground);
}

/* sites */
.sites-list {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.site-row {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: var(--font-text-body);
}

.site-rank {
  width: 14px;
  height: 14px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--color-muted);
  border-radius: 3px;
  font-size: 9px;
  font-weight: 600;
  color: var(--color-muted-foreground);
}

.site-domain {
  flex: 1;
  color: var(--color-foreground);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.site-count {
  font-family: 'JetBrains Mono', ui-monospace, monospace;
  font-size: var(--font-text-small);
  font-weight: 600;
  color: var(--color-muted-foreground);
}

/* empty */
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 20px;
  text-align: center;
}

.empty-state p {
  font-size: var(--font-text-body);
  color: var(--color-muted-foreground);
  margin: 0;
}

.empty-state .sub {
  font-size: var(--font-text-small);
  opacity: 0.7;
}

/* activity */
.activity-section {
  border-top: 1px solid var(--color-border);
  padding-top: 8px;
}

.activity-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  padding: 0;
  background: none;
  border: none;
  cursor: pointer;
}

.header-left {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: var(--font-text-small);
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.03em;
  color: var(--color-muted-foreground);
}

.header-left .count {
  font-family: 'JetBrains Mono', ui-monospace, monospace;
  padding: 1px 4px;
  background: var(--color-muted);
  border-radius: 3px;
}

.chevron {
  color: var(--color-muted-foreground);
  transition: transform 0.2s ease;
}

.chevron.collapsed {
  transform: rotate(180deg);
}

.activity-content {
  margin-top: 6px;
}

.activity-empty {
  padding: 10px;
  text-align: center;
  font-size: var(--font-text-body);
  color: var(--color-muted-foreground);
}

.activity-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.activity-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  background: var(--color-card);
  border: 1px solid var(--color-border);
  border-radius: 0.75rem;
  cursor: pointer;
  transition: all 0.15s ease;
}

.activity-item:hover {
  background: color-mix(in oklch, var(--color-muted) 70%, transparent);
}

.item-icon {
  width: 24px;
  height: 24px;
  border-radius: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.item-icon.email {
  background: color-mix(in oklch, var(--color-primary) 20%, transparent);
  color: var(--color-primary);
}

.item-icon.page {
  background: color-mix(in oklch, #22d3ee 20%, transparent);
  color: #22d3ee;
}

.item-content {
  flex: 1;
  min-width: 0;
}

.item-title {
  font-size: var(--font-text-body);
  font-weight: 500;
  line-height: 1.2;
  margin: 0;
  display: -webkit-box;
  -webkit-line-clamp: 1;
  line-clamp: 1;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.item-meta {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: var(--font-text-small);
  color: var(--color-muted-foreground);
}

.tag {
  padding: 2px 6px;
  border-radius: 4px;
  font-size: 10px;
  font-weight: 600;
}

.tag.violet {
  background: color-mix(in oklch, var(--color-primary) 20%, transparent);
  color: var(--color-primary);
}

.tag.cyan {
  background: color-mix(in oklch, #22d3ee 20%, transparent);
  color: #22d3ee;
}

.item-arrow {
  color: var(--color-muted-foreground);
  opacity: 0;
  flex-shrink: 0;
  transition: opacity 0.15s ease;
}

.activity-item:hover .item-arrow {
  opacity: 1;
}

.clear-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 3px;
  width: 100%;
  padding: 5px;
  margin-top: 4px;
  background: none;
  border: none;
  font-size: var(--font-text-small);
  color: color-mix(in oklch, var(--color-muted-foreground) 60%, transparent);
  cursor: pointer;
  transition: color 0.15s ease;
}

.clear-btn:hover {
  color: var(--color-destructive);
}

/* slide */
.slide-enter-active, .slide-leave-active {
  transition: all 0.2s ease;
  overflow: hidden;
}

.slide-enter-from, .slide-leave-to {
  opacity: 0;
  max-height: 0;
}

.slide-enter-to, .slide-leave-from {
  opacity: 1;
  max-height: 400px;
}
</style>
