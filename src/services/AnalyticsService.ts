import { databaseService, DB_CONFIGS } from './DatabaseService'
import { logger } from './LoggerService'

const log = logger.createScoped('AnalyticsService')

// ============ TYPES ============

// daily aggregated stats (key: YYYY-MM-DD)
export interface DailyStats {
  date: string
  
  // time tracking (seconds)
  timeInPanel: number
  timeInGmail: number
  timeInArticles: number
  timeInSettings: number
  timeInStats: number
  
  // hourly breakdown (24 slots, seconds per hour)
  hourlyActivity: number[]
  
  // interaction counts
  summariesGenerated: number
  chatMessagesSent: number
  chatMessagesReceived: number
  wordsLookedUp: number
  replySuggestionsGenerated: number
  replySuggestionsUsed: number
  
  // token usage
  tokensInput: number
  tokensOutput: number
  
  // content metrics
  emailsProcessed: number
  pagesProcessed: number
  pdfsProcessed: number
  totalWordsRead: number
  totalWordsSummarized: number
  
  // performance
  avgResponseTimeMs: number
  responseTimeSamples: number
  fastestResponseMs: number
  slowestResponseMs: number
  
  // domain tracking
  topDomains: Record<string, number>
  
  // model usage
  modelUsage: Record<string, number>
  
  // errors/retries
  errorCount: number
  
  usedChat: boolean
  usedSummary: boolean
  usedWordLookup: boolean
}

// individual session record
export interface SessionRecord {
  id: string
  startTime: number
  endTime: number
  duration: number
  context: 'gmail' | 'article' | 'other'
  tab: 'summary' | 'stats' | 'settings'
  url?: string
  domain?: string
  title?: string
  
  // what happened in this session
  summaryGenerated: boolean
  chatMessagesCount: number
  wordsLookedUp: number
  tokensUsed: number
}

// granular event log
export interface AnalyticsEvent {
  id: string
  timestamp: number
  type: EventType
  context?: string
  url?: string
  domain?: string
  metadata?: Record<string, unknown>
}

export type EventType = 
  | 'session_start' 
  | 'session_end'
  | 'summary_requested'
  | 'summary_completed'
  | 'summary_cached'
  | 'summary_failed'
  | 'chat_sent'
  | 'chat_received'
  | 'word_lookup'
  | 'word_lookup_cached'
  | 'reply_generated'
  | 'reply_used'
  | 'tab_switch'
  | 'model_switch'
  | 'provider_switch'
  | 'panel_opened'
  | 'panel_closed'
  | 'content_indexed'
  | 'search_performed'
  | 'pdf_processed'
  | 'error'

// user-facing analytics summary
export interface AnalyticsSummary {
  // overview
  totalTimeInPanel: number         // seconds
  totalSummaries: number
  totalChatMessages: number
  totalWordsLookedUp: number
  totalTokensInput: number
  totalTokensOutput: number
  totalWordsProcessed: number
  
  // streaks
  currentStreak: number            // consecutive days
  longestStreak: number
  lastActiveDate: string
  
  // time patterns
  peakHour: number                 // 0-23, most active hour
  peakDay: string                  // 'Monday', etc.
  avgSessionDuration: number       // seconds
  avgDailyUsage: number            // seconds per active day
  
  // productivity insights
  timeByContext: { gmail: number; articles: number; other: number }
  timeByTab: { summary: number; stats: number; settings: number }
  readingTimeSaved: number         // estimated time saved (seconds)
  
  // content insights
  totalEmailsProcessed: number
  totalPagesProcessed: number
  totalPdfsProcessed: number
  topDomains: Array<{ domain: string; count: number }>
  avgContentLength: number         // words
  
  // AI performance
  avgResponseTime: number          // ms
  fastestResponse: number          // ms
  modelBreakdown: Array<{ model: string; usage: number }>
  successRate: number              // 0-1
  
  // trends (for charts)
  dailyStats: DailyStats[]
  hourlyPatterns: number[]         // 24 slots, avg activity per hour
  weeklyPatterns: number[]         // 7 slots, avg activity per day of week
  dailyActivity: number[]          // last N days interaction counts (for sparkline)
  
  // recent activity
  recentSessions: SessionRecord[]
  recentEvents: AnalyticsEvent[]
}

// ============ CONSTANTS ============

const STORE_DAILY = 'daily_stats'
const STORE_SESSIONS = 'sessions'
const STORE_EVENTS = 'events'

const WORDS_PER_MINUTE_READING = 200
const AVG_WORDS_PER_EMAIL = 300
const AVG_WORDS_PER_ARTICLE = 1200

// ============ HELPERS ============

function estimateTokens(text: string): number {
  return Math.ceil(text.length / 4)
}

function getTodayKey(): string {
  return new Date().toISOString().split('T')[0]
}

function generateId(): string {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
}

function getCurrentHour(): number {
  return new Date().getHours()
}

function getDayOfWeek(): number {
  return new Date().getDay() // 0 = Sunday
}

function getDayName(dayIndex: number): string {
  return ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'][dayIndex]
}

function extractDomain(url: string): string {
  try {
    return new URL(url).hostname.replace('www.', '')
  } catch {
    return 'unknown'
  }
}

function createEmptyDailyStats(date: string): DailyStats {
  return {
    date,
    timeInPanel: 0,
    timeInGmail: 0,
    timeInArticles: 0,
    timeInSettings: 0,
    timeInStats: 0,
    hourlyActivity: new Array(24).fill(0),
    summariesGenerated: 0,
    chatMessagesSent: 0,
    chatMessagesReceived: 0,
    wordsLookedUp: 0,
    replySuggestionsGenerated: 0,
    replySuggestionsUsed: 0,
    tokensInput: 0,
    tokensOutput: 0,
    emailsProcessed: 0,
    pagesProcessed: 0,
    pdfsProcessed: 0,
    totalWordsRead: 0,
    totalWordsSummarized: 0,
    avgResponseTimeMs: 0,
    responseTimeSamples: 0,
    fastestResponseMs: Infinity,
    slowestResponseMs: 0,
    topDomains: {},
    modelUsage: {},
    errorCount: 0,
    usedChat: false,
    usedSummary: false,
    usedWordLookup: false
  }
}

// ============ SERVICE ============

class AnalyticsServiceClass {
  private currentSession: {
    id: string
    startTime: number
    context: string
    tab: string
    url?: string
    domain?: string
    chatCount: number
    wordsLookedUp: number
    tokensUsed: number
    summaryGenerated: boolean
  } | null = null
  
  private activeTimer: ReturnType<typeof setInterval> | null = null
  private lastTickTime = 0
  private notifyDebounceTimer: ReturnType<typeof setTimeout> | null = null

  // broadcast stats update to trigger UI refresh (debounced)
  private notifyStatsUpdated(): void {
    if (this.notifyDebounceTimer) {
      clearTimeout(this.notifyDebounceTimer)
    }
    this.notifyDebounceTimer = setTimeout(() => {
      chrome.runtime.sendMessage({ type: 'STATS_UPDATED' }).catch(() => {})
      this.notifyDebounceTimer = null
    }, 100) // 100ms debounce
  }

  // ============ DAILY STATS ============

  private async getTodayStats(): Promise<DailyStats> {
    const date = getTodayKey()
    const existing = await databaseService.get<DailyStats>(DB_CONFIGS.analytics, STORE_DAILY, date)
    
    if (existing) {
      // fix any missing fields from older versions
      if (!existing.hourlyActivity) existing.hourlyActivity = new Array(24).fill(0)
      if (!existing.topDomains) existing.topDomains = {}
      if (!existing.modelUsage) existing.modelUsage = {}
      return existing
    }
    
    return createEmptyDailyStats(date)
  }

  private async saveDailyStats(stats: DailyStats): Promise<void> {
    await databaseService.put(DB_CONFIGS.analytics, STORE_DAILY, stats)
  }

  private async incrementStat(key: keyof DailyStats, amount = 1): Promise<void> {
    const stats = await this.getTodayStats()
    const current = stats[key]
    if (typeof current === 'number') {
      (stats as unknown as Record<string, unknown>)[key] = current + amount
      await this.saveDailyStats(stats)
    }
  }

  private async setFlag(key: keyof DailyStats, value: boolean): Promise<void> {
    const stats = await this.getTodayStats()
    if (typeof stats[key] === 'boolean') {
      (stats as unknown as Record<string, unknown>)[key] = value
      await this.saveDailyStats(stats)
    }
  }

  // ============ SESSION TRACKING ============

  async startSession(
    context: 'gmail' | 'article' | 'other',
    tab = 'summary',
    url?: string
  ): Promise<void> {
    // end previous session if exists
    if (this.currentSession) {
      await this.endSession()
    }

    const domain = url ? extractDomain(url) : undefined

    this.currentSession = {
      id: generateId(),
      startTime: Date.now(),
      context,
      tab,
      url,
      domain,
      chatCount: 0,
      wordsLookedUp: 0,
      tokensUsed: 0,
      summaryGenerated: false
    }

    await this.logEvent('session_start', { context, tab, url, domain })
    await this.logEvent('panel_opened', { url })

    // track domain
    if (domain) {
      await this.trackDomain(domain)
    }

    // start timer for time tracking
    this.lastTickTime = Date.now()
    this.activeTimer = setInterval(() => this.tick(), 1000)
  }

  private async tick(): Promise<void> {
    if (!this.currentSession) return

    const now = Date.now()
    const elapsed = Math.floor((now - this.lastTickTime) / 1000)
    this.lastTickTime = now

    if (elapsed <= 0) return

    const stats = await this.getTodayStats()
    const hour = getCurrentHour()

    // update time counters
    stats.timeInPanel += elapsed
    stats.hourlyActivity[hour] = (stats.hourlyActivity[hour] || 0) + elapsed

    if (this.currentSession.context === 'gmail') {
      stats.timeInGmail += elapsed
    } else if (this.currentSession.context === 'article') {
      stats.timeInArticles += elapsed
    }

    if (this.currentSession.tab === 'settings') {
      stats.timeInSettings += elapsed
    } else if (this.currentSession.tab === 'stats') {
      stats.timeInStats += elapsed
    }

    await this.saveDailyStats(stats)
  }

  async endSession(): Promise<void> {
    if (!this.currentSession) return

    if (this.activeTimer) {
      clearInterval(this.activeTimer)
      this.activeTimer = null
    }

    // do final tick
    await this.tick()

    const endTime = Date.now()
    const duration = Math.floor((endTime - this.currentSession.startTime) / 1000)

    const session: SessionRecord = {
      id: this.currentSession.id,
      startTime: this.currentSession.startTime,
      endTime,
      duration,
      context: this.currentSession.context as 'gmail' | 'article' | 'other',
      tab: this.currentSession.tab as 'summary' | 'stats' | 'settings',
      url: this.currentSession.url,
      domain: this.currentSession.domain,
      summaryGenerated: this.currentSession.summaryGenerated,
      chatMessagesCount: this.currentSession.chatCount,
      wordsLookedUp: this.currentSession.wordsLookedUp,
      tokensUsed: this.currentSession.tokensUsed
    }

    await databaseService.put(DB_CONFIGS.analytics, STORE_SESSIONS, session)
    await this.logEvent('session_end', { ...session })
    await this.logEvent('panel_closed', { duration })

    this.currentSession = null
  }

  async updateSessionContext(context: 'gmail' | 'article' | 'other', url?: string): Promise<void> {
    if (this.currentSession) {
      this.currentSession.context = context
      if (url) {
        this.currentSession.url = url
        this.currentSession.domain = extractDomain(url)
        await this.trackDomain(this.currentSession.domain)
      }
    }
  }

  async trackTabSwitch(newTab: 'summary' | 'stats' | 'settings'): Promise<void> {
    const prevTab = this.currentSession?.tab
    if (this.currentSession) {
      this.currentSession.tab = newTab
    }
    await this.logEvent('tab_switch', { from: prevTab, to: newTab })
  }

  // ============ CONTENT TRACKING ============

  async trackSummary(opts: {
    type: 'email' | 'page'
    cached: boolean
    responseTimeMs: number
    wordsIn: number
    wordsOut: number
    tokensOut: number
    model?: string
    url?: string
  }): Promise<void> {
    const stats = await this.getTodayStats()

    stats.summariesGenerated++
    stats.tokensOutput += opts.tokensOut
    stats.totalWordsRead += opts.wordsIn
    stats.totalWordsSummarized += opts.wordsOut
    stats.usedSummary = true

    if (opts.type === 'email') {
      stats.emailsProcessed++
    } else {
      stats.pagesProcessed++
    }

    // response time tracking
    if (!opts.cached) {
      stats.responseTimeSamples++
      const totalTime = stats.avgResponseTimeMs * (stats.responseTimeSamples - 1)
      stats.avgResponseTimeMs = (totalTime + opts.responseTimeMs) / stats.responseTimeSamples
      
      if (opts.responseTimeMs < stats.fastestResponseMs || stats.fastestResponseMs === Infinity) {
        stats.fastestResponseMs = opts.responseTimeMs
      }
      if (opts.responseTimeMs > stats.slowestResponseMs) {
        stats.slowestResponseMs = opts.responseTimeMs
      }
    }

    // model tracking
    if (opts.model) {
      stats.modelUsage[opts.model] = (stats.modelUsage[opts.model] || 0) + 1
    }

    // domain tracking
    if (opts.url) {
      const domain = extractDomain(opts.url)
      stats.topDomains[domain] = (stats.topDomains[domain] || 0) + 1
    }

    await this.saveDailyStats(stats)

    if (this.currentSession) {
      this.currentSession.summaryGenerated = true
      this.currentSession.tokensUsed += opts.tokensOut
    }

    await this.logEvent(opts.cached ? 'summary_cached' : 'summary_completed', {
      type: opts.type,
      responseTimeMs: opts.responseTimeMs,
      wordsIn: opts.wordsIn,
      wordsOut: opts.wordsOut,
      model: opts.model,
      url: opts.url
    })
    
    this.notifyStatsUpdated()
  }

  async trackSummaryFailed(error: string, type: 'email' | 'page'): Promise<void> {
    await this.incrementStat('errorCount')
    await this.logEvent('summary_failed', { error, type })
  }

  async trackChat(role: 'user' | 'assistant', text: string, responseTimeMs?: number): Promise<void> {
    const tokens = estimateTokens(text)
    const stats = await this.getTodayStats()

    if (role === 'user') {
      stats.chatMessagesSent++
      stats.tokensInput += tokens
      stats.usedChat = true
    } else {
      stats.chatMessagesReceived++
      stats.tokensOutput += tokens
      
      // track AI response time
      if (responseTimeMs) {
        stats.responseTimeSamples++
        const totalTime = stats.avgResponseTimeMs * (stats.responseTimeSamples - 1)
        stats.avgResponseTimeMs = (totalTime + responseTimeMs) / stats.responseTimeSamples
      }
    }

    await this.saveDailyStats(stats)

    if (this.currentSession) {
      this.currentSession.chatCount++
      this.currentSession.tokensUsed += tokens
    }

    await this.logEvent(role === 'user' ? 'chat_sent' : 'chat_received', {
      tokens,
      length: text.length,
      responseTimeMs
    })
    
    this.notifyStatsUpdated()
  }

  async trackWordLookup(word: string, cached: boolean): Promise<void> {
    await this.incrementStat('wordsLookedUp')
    await this.setFlag('usedWordLookup', true)

    if (this.currentSession) {
      this.currentSession.wordsLookedUp++
    }

    await this.logEvent(cached ? 'word_lookup_cached' : 'word_lookup', { word })
    
    this.notifyStatsUpdated()
  }

  async trackReplyGenerated(count: number): Promise<void> {
    const stats = await this.getTodayStats()
    stats.replySuggestionsGenerated += count
    await this.saveDailyStats(stats)
    await this.logEvent('reply_generated', { count })
  }

  async trackReplyUsed(): Promise<void> {
    await this.incrementStat('replySuggestionsUsed')
    await this.logEvent('reply_used', {})
  }

  async trackModelSwitch(from: string, to: string): Promise<void> {
    await this.logEvent('model_switch', { from, to })
  }

  async trackProviderSwitch(from: string, to: string): Promise<void> {
    await this.logEvent('provider_switch', { from, to })
  }

  async trackContentIndexed(url: string, chunks: number): Promise<void> {
    await this.logEvent('content_indexed', { url, chunks })
  }

  async trackSearch(query: string, resultsCount: number): Promise<void> {
    await this.logEvent('search_performed', { query, resultsCount })
  }

  async trackPdfProcessed(pages: number, chunks: number): Promise<void> {
    await this.logEvent('pdf_processed', { pages, chunks })
  }

  async trackError(error: string, context?: string): Promise<void> {
    await this.incrementStat('errorCount')
    await this.logEvent('error', { error, context })
  }

  async trackRetry(operation: string): Promise<void> {
    await this.logEvent('error', { operation, isRetry: true })
  }

  private async trackDomain(domain: string): Promise<void> {
    const stats = await this.getTodayStats()
    stats.topDomains[domain] = (stats.topDomains[domain] || 0) + 1
    await this.saveDailyStats(stats)
  }

  // ============ EVENT LOGGING ============

  private async logEvent(type: EventType, metadata?: Record<string, unknown>): Promise<void> {
    const event: AnalyticsEvent = {
      id: generateId(),
      timestamp: Date.now(),
      type,
      context: this.currentSession?.context,
      url: this.currentSession?.url,
      domain: this.currentSession?.domain,
      metadata
    }

    try {
      await databaseService.put(DB_CONFIGS.analytics, STORE_EVENTS, event)
    } catch (err) {
      log.error('failed to log event:', err)
    }
  }

  // ============ ANALYTICS RETRIEVAL ============

  async getAnalyticsSummary(days = 30): Promise<AnalyticsSummary> {
    try {
      const allDaily = await databaseService.getAll<DailyStats>(DB_CONFIGS.analytics, STORE_DAILY)
      const allSessions = await databaseService.getAll<SessionRecord>(DB_CONFIGS.analytics, STORE_SESSIONS)
      const allEvents = await databaseService.getAll<AnalyticsEvent>(DB_CONFIGS.analytics, STORE_EVENTS)

      // filter to requested days
      const cutoff = new Date()
      cutoff.setDate(cutoff.getDate() - days)
      const cutoffStr = cutoff.toISOString().split('T')[0]
      const cutoffMs = cutoff.getTime()

      const recentDaily = allDaily
        .filter(d => d.date >= cutoffStr)
        .sort((a, b) => a.date.localeCompare(b.date))

      const recentSessions = allSessions
        .filter(s => s.startTime >= cutoffMs)
        .sort((a, b) => b.startTime - a.startTime)
        .slice(0, 50)

      const recentEvents = allEvents
        .filter(e => e.timestamp >= cutoffMs)
        .sort((a, b) => b.timestamp - a.timestamp)
        .slice(0, 100)

      // calculate streaks
      const { currentStreak, longestStreak, lastActiveDate } = this.calculateStreaks(allDaily)

      // aggregate totals
      const totals = recentDaily.reduce((acc, day) => ({
        timeInPanel: acc.timeInPanel + day.timeInPanel,
        summaries: acc.summaries + day.summariesGenerated,
        chatMessages: acc.chatMessages + day.chatMessagesSent + day.chatMessagesReceived,
        wordsLookedUp: acc.wordsLookedUp + day.wordsLookedUp,
        tokensInput: acc.tokensInput + day.tokensInput,
        tokensOutput: acc.tokensOutput + day.tokensOutput,
        wordsProcessed: acc.wordsProcessed + day.totalWordsRead,
        wordsSummarized: acc.wordsSummarized + day.totalWordsSummarized,
        gmail: acc.gmail + day.timeInGmail,
        articles: acc.articles + day.timeInArticles,
        settings: acc.settings + day.timeInSettings,
        stats: acc.stats + day.timeInStats,
        emails: acc.emails + day.emailsProcessed,
        pages: acc.pages + day.pagesProcessed,
        pdfs: acc.pdfs + (day.pdfsProcessed || 0),
        errors: acc.errors + day.errorCount
      }), {
        timeInPanel: 0, summaries: 0, chatMessages: 0, wordsLookedUp: 0,
        tokensInput: 0, tokensOutput: 0, wordsProcessed: 0, wordsSummarized: 0,
        gmail: 0, articles: 0, settings: 0, stats: 0, emails: 0, pages: 0, pdfs: 0,
        errors: 0
      })

      // hourly patterns (average across all days)
      const hourlyPatterns = new Array(24).fill(0)
      const daysWithData = recentDaily.filter(d => d.hourlyActivity?.some(h => h > 0)).length
      if (daysWithData > 0) {
        recentDaily.forEach(day => {
          if (day.hourlyActivity) {
            day.hourlyActivity.forEach((val, i) => {
              hourlyPatterns[i] += val / daysWithData
            })
          }
        })
      }

      // weekly patterns
      const weeklyPatterns = new Array(7).fill(0)
      const weekCounts = new Array(7).fill(0)
      recentDaily.forEach(day => {
        const dayOfWeek = new Date(day.date).getDay()
        weeklyPatterns[dayOfWeek] += day.timeInPanel
        weekCounts[dayOfWeek]++
      })
      weeklyPatterns.forEach((val, i) => {
        if (weekCounts[i] > 0) {
          weeklyPatterns[i] = val / weekCounts[i]
        }
      })

      // peak hour and day
      const peakHour = hourlyPatterns.indexOf(Math.max(...hourlyPatterns))
      const peakDayIndex = weeklyPatterns.indexOf(Math.max(...weeklyPatterns))
      const peakDay = getDayName(peakDayIndex)

      // top domains aggregate
      const domainTotals: Record<string, number> = {}
      recentDaily.forEach(day => {
        if (day.topDomains) {
          Object.entries(day.topDomains).forEach(([domain, count]) => {
            domainTotals[domain] = (domainTotals[domain] || 0) + count
          })
        }
      })
      const topDomains = Object.entries(domainTotals)
        .map(([domain, count]) => ({ domain, count }))
        .sort((a, b) => b.count - a.count)
        .slice(0, 10)

      // model usage aggregate
      const modelTotals: Record<string, number> = {}
      recentDaily.forEach(day => {
        if (day.modelUsage) {
          Object.entries(day.modelUsage).forEach(([model, count]) => {
            modelTotals[model] = (modelTotals[model] || 0) + count
          })
        }
      })
      const modelBreakdown = Object.entries(modelTotals)
        .map(([model, usage]) => ({ model, usage }))
        .sort((a, b) => b.usage - a.usage)

      // response time (weighted average)
      let totalSamples = 0
      let weightedTotal = 0
      let fastestResponse = Infinity
      recentDaily.forEach(day => {
        if (day.responseTimeSamples > 0) {
          weightedTotal += day.avgResponseTimeMs * day.responseTimeSamples
          totalSamples += day.responseTimeSamples
          if (day.fastestResponseMs < fastestResponse && day.fastestResponseMs !== Infinity) {
            fastestResponse = day.fastestResponseMs
          }
        }
      })
      const avgResponseTime = totalSamples > 0 ? weightedTotal / totalSamples : 0

      // session metrics
      const totalSessionDuration = recentSessions.reduce((sum, s) => sum + s.duration, 0)
      const avgSessionDuration = recentSessions.length > 0
        ? totalSessionDuration / recentSessions.length
        : 0

      const activeDays = recentDaily.filter(d => d.timeInPanel > 0).length
      const avgDailyUsage = activeDays > 0 ? totals.timeInPanel / activeDays : 0

      // reading time saved
      const readingTimeSaved = totals.wordsProcessed / WORDS_PER_MINUTE_READING * 60

      // success rate
      const totalAttempts = totals.summaries + totals.errors
      const successRate = totalAttempts > 0 ? totals.summaries / totalAttempts : 1

      // chat to summary ratio
      const chatToSummaryRatio = totals.summaries > 0 
        ? totals.chatMessages / totals.summaries 
        : 0

      // average content length
      const avgContentLength = totals.summaries > 0
        ? totals.wordsProcessed / totals.summaries
        : 0

      // tokens per chat
      const avgTokensPerChat = totals.chatMessages > 0
        ? (totals.tokensInput + totals.tokensOutput) / totals.chatMessages
        : 0

      return {
        totalTimeInPanel: totals.timeInPanel,
        totalSummaries: totals.summaries,
        totalChatMessages: totals.chatMessages,
        totalWordsLookedUp: totals.wordsLookedUp,
        totalTokensInput: totals.tokensInput,
        totalTokensOutput: totals.tokensOutput,
        totalWordsProcessed: totals.wordsProcessed,
        currentStreak,
        longestStreak,
        lastActiveDate,
        peakHour,
        peakDay,
        avgSessionDuration,
        avgDailyUsage,
        timeByContext: {
          gmail: totals.gmail,
          articles: totals.articles,
          other: Math.max(0, totals.timeInPanel - totals.gmail - totals.articles)
        },
        timeByTab: {
          summary: Math.max(0, totals.timeInPanel - totals.settings - totals.stats),
          stats: totals.stats,
          settings: totals.settings
        },
        readingTimeSaved,
        totalEmailsProcessed: totals.emails,
        totalPagesProcessed: totals.pages,
        totalPdfsProcessed: totals.pdfs,
        topDomains,
        avgContentLength,
        avgResponseTime,
        fastestResponse: fastestResponse === Infinity ? 0 : fastestResponse,
        modelBreakdown,
        successRate,
        dailyStats: recentDaily,
        hourlyPatterns,
        weeklyPatterns,
        dailyActivity: recentDaily.map(d => d.summariesGenerated + d.chatMessagesSent + d.wordsLookedUp),
        recentSessions,
        recentEvents
      }
    } catch (err) {
      log.error('getAnalyticsSummary failed:', err)
      return this.getEmptySummary()
    }
  }

  private calculateStreaks(allDaily: DailyStats[]): {
    currentStreak: number
    longestStreak: number
    lastActiveDate: string
  } {
    if (allDaily.length === 0) {
      return { currentStreak: 0, longestStreak: 0, lastActiveDate: '' }
    }

    const sorted = allDaily
      .filter(d => d.timeInPanel > 0)
      .sort((a, b) => b.date.localeCompare(a.date))

    if (sorted.length === 0) {
      return { currentStreak: 0, longestStreak: 0, lastActiveDate: '' }
    }

    const lastActiveDate = sorted[0].date
    let currentStreak = 0
    let longestStreak = 0
    let tempStreak = 0
    let prevDate: Date | null = null

    // calculate current streak (from today backwards)
    const today = getTodayKey()
    const yesterday = new Date()
    yesterday.setDate(yesterday.getDate() - 1)
    const yesterdayKey = yesterday.toISOString().split('T')[0]

    // check if active today or yesterday to start counting
    const isActiveToday = sorted.some(d => d.date === today)
    const isActiveYesterday = sorted.some(d => d.date === yesterdayKey)

    if (isActiveToday || isActiveYesterday) {
      let checkDate = isActiveToday ? new Date(today) : yesterday

      for (let i = 0; i < 365; i++) {
        const dateKey = checkDate.toISOString().split('T')[0]
        if (sorted.some(d => d.date === dateKey)) {
          currentStreak++
          checkDate.setDate(checkDate.getDate() - 1)
        } else {
          break
        }
      }
    }

    // calculate longest streak
    sorted.sort((a, b) => a.date.localeCompare(b.date))
    for (const day of sorted) {
      const currentDate = new Date(day.date)
      
      if (prevDate) {
        const diffDays = Math.round((currentDate.getTime() - prevDate.getTime()) / (1000 * 60 * 60 * 24))
        if (diffDays === 1) {
          tempStreak++
        } else {
          tempStreak = 1
        }
      } else {
        tempStreak = 1
      }

      longestStreak = Math.max(longestStreak, tempStreak)
      prevDate = currentDate
    }

    return { currentStreak, longestStreak, lastActiveDate }
  }

  private getEmptySummary(): AnalyticsSummary {
    return {
      totalTimeInPanel: 0,
      totalSummaries: 0,
      totalChatMessages: 0,
      totalWordsLookedUp: 0,
      totalTokensInput: 0,
      totalTokensOutput: 0,
      totalWordsProcessed: 0,
      currentStreak: 0,
      longestStreak: 0,
      lastActiveDate: '',
      peakHour: 0,
      peakDay: 'Monday',
      avgSessionDuration: 0,
      avgDailyUsage: 0,
      timeByContext: { gmail: 0, articles: 0, other: 0 },
      timeByTab: { summary: 0, stats: 0, settings: 0 },
      readingTimeSaved: 0,
      totalEmailsProcessed: 0,
      totalPagesProcessed: 0,
      totalPdfsProcessed: 0,
      topDomains: [],
      avgContentLength: 0,
      avgResponseTime: 0,
      fastestResponse: 0,
      modelBreakdown: [],
      successRate: 1,
      dailyStats: [],
      hourlyPatterns: new Array(24).fill(0),
      weeklyPatterns: new Array(7).fill(0),
      dailyActivity: [],
      recentSessions: [],
      recentEvents: []
    }
  }

  // ============ DATA MANAGEMENT ============

  async pruneOldData(keepDays = 90): Promise<void> {
    try {
      const cutoff = new Date()
      cutoff.setDate(cutoff.getDate() - keepDays)
      const cutoffMs = cutoff.getTime()
      const cutoffStr = cutoff.toISOString().split('T')[0]

      const allDaily = await databaseService.getAll<DailyStats>(DB_CONFIGS.analytics, STORE_DAILY)
      for (const day of allDaily) {
        if (day.date < cutoffStr) {
          await databaseService.delete(DB_CONFIGS.analytics, STORE_DAILY, day.date)
        }
      }

      const allEvents = await databaseService.getAll<AnalyticsEvent>(DB_CONFIGS.analytics, STORE_EVENTS)
      for (const event of allEvents) {
        if (event.timestamp < cutoffMs) {
          await databaseService.delete(DB_CONFIGS.analytics, STORE_EVENTS, event.id)
        }
      }

      const allSessions = await databaseService.getAll<SessionRecord>(DB_CONFIGS.analytics, STORE_SESSIONS)
      for (const session of allSessions) {
        if (session.startTime < cutoffMs) {
          await databaseService.delete(DB_CONFIGS.analytics, STORE_SESSIONS, session.id)
        }
      }

      log.log(`pruned analytics data older than ${keepDays} days`)
    } catch (err) {
      log.error('pruneOldData failed:', err)
    }
  }

  async exportData(): Promise<{
    dailyStats: DailyStats[]
    sessions: SessionRecord[]
    events: AnalyticsEvent[]
  }> {
    const [dailyStats, sessions, events] = await Promise.all([
      databaseService.getAll<DailyStats>(DB_CONFIGS.analytics, STORE_DAILY),
      databaseService.getAll<SessionRecord>(DB_CONFIGS.analytics, STORE_SESSIONS),
      databaseService.getAll<AnalyticsEvent>(DB_CONFIGS.analytics, STORE_EVENTS)
    ])
    return { dailyStats, sessions, events }
  }

  async clearAllData(): Promise<void> {
    await Promise.all([
      databaseService.clear(DB_CONFIGS.analytics, STORE_DAILY),
      databaseService.clear(DB_CONFIGS.analytics, STORE_SESSIONS),
      databaseService.clear(DB_CONFIGS.analytics, STORE_EVENTS)
    ])
    log.log('cleared all analytics data')
  }

  // seed test data for development/testing
  async seedTestData(days = 30): Promise<void> {
    log.log(`seeding ${days} days of test data...`)
    
    for (let i = 0; i < days; i++) {
      const date = new Date()
      date.setDate(date.getDate() - (days - i - 1))
      const dateKey = date.toISOString().split('T')[0]
      
      const wave = Math.sin(i / 4) * 3 + Math.random() * 4
      const summaries = Math.floor(6 + wave)
      const chatMessages = Math.floor(8 + wave * 1.5)
      
      const dailyStats: DailyStats = {
        date: dateKey,
        timeInPanel: Math.floor(Math.random() * 3600) + 1800,
        timeInGmail: Math.floor(Math.random() * 1200),
        timeInArticles: Math.floor(Math.random() * 1800),
        timeInSettings: 120,
        timeInStats: 180,
        hourlyActivity: Array.from({ length: 24 }, () => Math.floor(Math.random() * 300)),
        summariesGenerated: summaries,
        chatMessagesSent: chatMessages,
        chatMessagesReceived: chatMessages,
        wordsLookedUp: Math.floor(Math.random() * 4),
        replySuggestionsGenerated: Math.floor(Math.random() * 3),
        replySuggestionsUsed: Math.floor(Math.random() * 2),
        tokensInput: Math.floor(Math.random() * 5000) + 2000,
        tokensOutput: Math.floor(Math.random() * 3000) + 1000,
        emailsProcessed: Math.floor(Math.random() * 5) + 1,
        pagesProcessed: Math.floor(Math.random() * 6) + 2,
        pdfsProcessed: Math.floor(Math.random() * 2),
        totalWordsRead: Math.floor(Math.random() * 8000) + 3000,
        totalWordsSummarized: Math.floor(Math.random() * 800) + 300,
        avgResponseTimeMs: Math.floor(Math.random() * 600) + 250,
        responseTimeSamples: 10,
        fastestResponseMs: 150,
        slowestResponseMs: 1200,
        topDomains: { 'github.com': Math.floor(Math.random() * 8), 'medium.com': Math.floor(Math.random() * 5) },
        modelUsage: { 'gemini-nano': summaries + chatMessages },
        errorCount: 0,
        usedChat: true,
        usedSummary: true,
        usedWordLookup: Math.random() > 0.5
      }
      
      await databaseService.put(DB_CONFIGS.analytics, STORE_DAILY, dailyStats)
    }
    
    log.log(`seeded ${days} days of test data`)
  }
}

export const analyticsService = new AnalyticsServiceClass()
