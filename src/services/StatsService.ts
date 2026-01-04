import { databaseService, DB_CONFIGS } from './DatabaseService'
import { logger } from './LoggerService'
import type { EmailSession, AppChatMessage, AppPageSummary } from '@/types'
import type { TabSessionEntry } from './CacheService'

const log = logger.createScoped('StatsService')

export interface UsageStats {
  // content stats
  totalEmails: number
  totalPages: number
  totalInteractions: number
  
  // time savings (minutes)
  estimatedTimeSaved: number
  
  // activity counts
  thisWeek: number
  today: number
  
  // engagement
  totalChatMessages: number
  avgResponseTime: number
}

export interface ActivityItem {
  id: string
  type: 'email' | 'page'
  title: string
  timestamp: number
  hasSummary: boolean
  hasChat: boolean
  chatCount: number
  timeSaved?: number // minutes
}

const STORE_EMAIL_SESSIONS = 'email_sessions'
const STORE_TAB_SESSIONS = 'tab_sessions'

// time saved estimates
const MIN_PER_EMAIL = 2.5
const WORDS_PER_MINUTE = 200

class StatsServiceClass {
  async getStats(): Promise<UsageStats> {
    try {
      const [emailSessions, tabSessions] = await Promise.all([
        databaseService.getAll<EmailSession>(DB_CONFIGS.cache, STORE_EMAIL_SESSIONS),
        databaseService.getAll<TabSessionEntry>(DB_CONFIGS.cache, STORE_TAB_SESSIONS)
      ])

      const now = Date.now()
      const weekAgo = now - (7 * 24 * 60 * 60 * 1000)
      const todayStart = new Date().setHours(0, 0, 0, 0)

      // count emails with content (summary or chat)
      const emailsWithContent = emailSessions.filter(s => 
        s.summary || (s.chatMessages && s.chatMessages.length > 0)
      )
      
      // count pages with content
      const pagesWithContent = tabSessions.filter(s => 
        s.pageSummary || (s.chatMessages && s.chatMessages.length > 0)
      )

      // activity this week / today
      const thisWeek = [...emailsWithContent, ...pagesWithContent]
        .filter(s => s.timestamp > weekAgo).length
      
      const today = [...emailsWithContent, ...pagesWithContent]
        .filter(s => s.timestamp > todayStart).length

      // chat messages count
      const emailChatCount = emailSessions.reduce((sum, s) => 
        sum + (s.chatMessages?.length || 0), 0)
      const pageChatCount = tabSessions.reduce((sum, s) => 
        sum + (s.chatMessages?.length || 0), 0)

      // time saved calculation
      const emailTimeSaved = emailsWithContent.filter(s => s.summary).length * MIN_PER_EMAIL
      const pageTimeSaved = pagesWithContent
        .filter(s => s.pageSummary)
        .reduce((sum, s) => {
          const wordCount = s.pageSummary?.wordCount || 500
          return sum + (wordCount / WORDS_PER_MINUTE)
        }, 0)

      // avg response time from summaries
      const responseTimes: number[] = []
      emailSessions.forEach(s => {
        if (s.summary?.time_ms) responseTimes.push(s.summary.time_ms)
      })
      tabSessions.forEach(s => {
        if (s.pageSummary?.timing?.total) responseTimes.push(s.pageSummary.timing.total)
      })
      const avgResponseTime = responseTimes.length > 0
        ? responseTimes.reduce((a, b) => a + b, 0) / responseTimes.length
        : 0

      return {
        totalEmails: emailsWithContent.length,
        totalPages: pagesWithContent.length,
        totalInteractions: emailsWithContent.length + pagesWithContent.length,
        estimatedTimeSaved: Math.round(emailTimeSaved + pageTimeSaved),
        thisWeek,
        today,
        totalChatMessages: emailChatCount + pageChatCount,
        avgResponseTime: Math.round(avgResponseTime)
      }
    } catch (err) {
      log.error('getStats failed:', err)
      return {
        totalEmails: 0,
        totalPages: 0,
        totalInteractions: 0,
        estimatedTimeSaved: 0,
        thisWeek: 0,
        today: 0,
        totalChatMessages: 0,
        avgResponseTime: 0
      }
    }
  }

  async getRecentActivity(limit = 10): Promise<ActivityItem[]> {
    try {
      const [emailSessions, tabSessions] = await Promise.all([
        databaseService.getAll<EmailSession>(DB_CONFIGS.cache, STORE_EMAIL_SESSIONS),
        databaseService.getAll<TabSessionEntry>(DB_CONFIGS.cache, STORE_TAB_SESSIONS)
      ])

      const activities: ActivityItem[] = []

      // map email sessions to activity items
      emailSessions
        .filter(s => s.summary || (s.chatMessages && s.chatMessages.length > 0))
        .forEach(session => {
          activities.push({
            id: session.emailId,
            type: 'email',
            title: session.summary?.summary?.slice(0, 80) || 'Email conversation',
            timestamp: session.timestamp,
            hasSummary: !!session.summary,
            hasChat: (session.chatMessages?.length || 0) > 0,
            chatCount: session.chatMessages?.length || 0,
            timeSaved: session.summary ? MIN_PER_EMAIL : undefined
          })
        })

      // map page sessions to activity items
      tabSessions
        .filter(s => s.pageSummary || (s.chatMessages && s.chatMessages.length > 0))
        .forEach(session => {
          const wordCount = session.pageSummary?.wordCount || 0
          activities.push({
            id: session.url,
            type: 'page',
            title: session.pageSummary?.title || extractHostname(session.url),
            timestamp: session.timestamp,
            hasSummary: !!session.pageSummary,
            hasChat: (session.chatMessages?.length || 0) > 0,
            chatCount: session.chatMessages?.length || 0,
            timeSaved: wordCount > 0 ? wordCount / WORDS_PER_MINUTE : undefined
          })
        })

      // sort by timestamp desc and limit
      return activities
        .sort((a, b) => b.timestamp - a.timestamp)
        .slice(0, limit)
    } catch (err) {
      log.error('getRecentActivity failed:', err)
      return []
    }
  }
}

function extractHostname(url: string): string {
  try {
    return new URL(url).hostname.replace('www.', '')
  } catch {
    return url.slice(0, 40)
  }
}

export const statsService = new StatsServiceClass()
