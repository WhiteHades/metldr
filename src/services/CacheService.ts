import { databaseService, DB_CONFIGS } from './DatabaseService'
import { logger } from './LoggerService'
import type { AppPageSummary, AppChatMessage } from '@/types'

const log = logger.createScoped('CacheService')
const STORE_SUMMARIES = 'summaries'
const STORE_PAGE_CACHE = 'page_cache'
const STORE_REPLY_SUGGESTIONS = 'reply_suggestions'
const STORE_TAB_SESSIONS = 'tab_sessions'

interface CacheEntry<T> {
  timestamp: number
  ttl: number
  data: T
}

interface EmailSummaryEntry extends CacheEntry<unknown> {
  emailId: string
  summary: unknown
}

interface PageSummaryEntry extends CacheEntry<unknown> {
  url: string
  summary: unknown
}

interface ReplySuggestionsEntry extends CacheEntry<unknown> {
  emailId: string
  suggestions: unknown
}

export interface TabSessionEntry {
  url: string
  timestamp: number
  chatMessages: AppChatMessage[]
  pageSummary: AppPageSummary | null
  summaryCollapsed: boolean
}

export class CacheService {
  private isExpired(entry: { timestamp?: number; ttl?: number }): boolean {
    if (!entry.timestamp || !entry.ttl) return false
    return Date.now() > entry.timestamp + entry.ttl
  }

  async getEmailSummary(emailId: string): Promise<unknown> {
    try {
      const result = await databaseService.get<EmailSummaryEntry>(DB_CONFIGS.cache, STORE_SUMMARIES, emailId)
      if (!result) return null

      if (this.isExpired(result)) {
        this.deleteEmailSummary(emailId).catch(() => {})
        return null
      }

      return result.summary
    } catch (err) {
      log.error('getEmailSummary failed', (err as Error).message)
      return null
    }
  }

  async setEmailSummary(emailId: string, summary: unknown, ttlMs = 7 * 24 * 60 * 60 * 1000): Promise<void> {
    try {
      await databaseService.put(DB_CONFIGS.cache, STORE_SUMMARIES, {
        emailId,
        summary,
        timestamp: Date.now(),
        ttl: ttlMs
      })
      chrome.runtime.sendMessage({ type: 'SUMMARY_ADDED', emailId }).catch(() => {})
    } catch (err) {
      log.error('setEmailSummary failed', (err as Error).message)
    }
  }

  async deleteEmailSummary(emailId: string): Promise<void> {
    try {
      await databaseService.delete(DB_CONFIGS.cache, STORE_SUMMARIES, emailId)
    } catch (err) {
      log.error('deleteEmailSummary failed', (err as Error).message)
    }
  }

  async getPageSummary(url: string): Promise<unknown> {
    try {
      const result = await databaseService.get<PageSummaryEntry>(DB_CONFIGS.cache, STORE_PAGE_CACHE, url)
      if (!result) return null

      if (this.isExpired(result)) {
        this.deletePageSummary(url).catch(() => {})
        return null
      }

      return result.summary
    } catch (err) {
      log.error('getPageSummary failed', (err as Error).message)
      return null
    }
  }

  async setPageSummary(url: string, summary: unknown, ttlSeconds = 3600): Promise<void> {
    try {
      await databaseService.put(DB_CONFIGS.cache, STORE_PAGE_CACHE, {
        url,
        summary,
        timestamp: Date.now(),
        ttl: ttlSeconds * 1000
      })
    } catch (err) {
      log.error('setPageSummary failed', (err as Error).message)
    }
  }

  async deletePageSummary(url: string): Promise<void> {
    try {
      await databaseService.delete(DB_CONFIGS.cache, STORE_PAGE_CACHE, url)
    } catch (err) {
      log.error('deletePageSummary failed', (err as Error).message)
    }
  }

  async clearAll(): Promise<void> {
    try {
      await Promise.all([
        databaseService.clear(DB_CONFIGS.cache, STORE_SUMMARIES),
        databaseService.clear(DB_CONFIGS.cache, STORE_PAGE_CACHE),
        databaseService.clear(DB_CONFIGS.cache, STORE_REPLY_SUGGESTIONS),
        databaseService.clear(DB_CONFIGS.cache, STORE_TAB_SESSIONS)
      ])
    } catch (err) {
      log.error('clearAll failed', (err as Error).message)
    }
  }

  async getReplySuggestions(emailId: string): Promise<unknown> {
    try {
      const result = await databaseService.get<ReplySuggestionsEntry>(DB_CONFIGS.cache, STORE_REPLY_SUGGESTIONS, emailId)
      if (!result) return null

      if (this.isExpired(result)) {
        this.deleteReplySuggestions(emailId).catch(() => {})
        return null
      }

      return result.suggestions
    } catch (err) {
      log.error('getReplySuggestions failed', (err as Error).message)
      return null
    }
  }

  async setReplySuggestions(emailId: string, suggestions: unknown, ttlMs = 7 * 24 * 60 * 60 * 1000): Promise<void> {
    try {
      await databaseService.put(DB_CONFIGS.cache, STORE_REPLY_SUGGESTIONS, {
        emailId,
        suggestions,
        timestamp: Date.now(),
        ttl: ttlMs
      })
    } catch (err) {
      log.error('setReplySuggestions failed', (err as Error).message)
    }
  }

  async deleteReplySuggestions(emailId: string): Promise<void> {
    try {
      await databaseService.delete(DB_CONFIGS.cache, STORE_REPLY_SUGGESTIONS, emailId)
    } catch (err) {
      log.error('deleteReplySuggestions failed', (err as Error).message)
    }
  }

  async getTabSession(url: string): Promise<TabSessionEntry | null> {
    try {
      const result = await databaseService.get<TabSessionEntry>(DB_CONFIGS.cache, STORE_TAB_SESSIONS, url)
      return result || null
    } catch (err) {
      log.error('getTabSession failed', (err as Error).message)
      return null
    }
  }

  async setTabSession(
    url: string, 
    chatMessages: AppChatMessage[], 
    pageSummary: AppPageSummary | null, 
    summaryCollapsed: boolean
  ): Promise<void> {
    try {
      const plainMessages = JSON.parse(JSON.stringify(chatMessages))
      const plainSummary = pageSummary ? JSON.parse(JSON.stringify(pageSummary)) : null
      
      await databaseService.put(DB_CONFIGS.cache, STORE_TAB_SESSIONS, {
        url,
        chatMessages: plainMessages,
        pageSummary: plainSummary,
        summaryCollapsed,
        timestamp: Date.now()
      })
    } catch (err) {
      log.error('setTabSession failed', (err as Error).message)
    }
  }

  async deleteTabSession(url: string): Promise<void> {
    try {
      await databaseService.delete(DB_CONFIGS.cache, STORE_TAB_SESSIONS, url)
    } catch (err) {
      log.error('deleteTabSession failed', (err as Error).message)
    }
  }
}

export const cacheService = new CacheService()
