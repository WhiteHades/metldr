import type { AppChatMessage, AppPageSummary, EmailSession } from '@/types'

export interface CachedEmailSession extends EmailSession {
  emailCount?: number
}

export interface TabSessionEntry {
  url: string
  timestamp: number
  chatMessages: AppChatMessage[]
  pageSummary: AppPageSummary | null
  summaryCollapsed: boolean
}

export interface EmailHeader {
  emailId: string
  timestamp: number
  ttl: number
  emailCount?: number
  chatCount: number
  hasSummary: boolean
  summaryTitle: string
  summaryTimeMs: number
  recentOrder?: number
}

export interface TabHeader {
  url: string
  timestamp: number
  summaryCollapsed: boolean
  chatCount: number
  hasSummary: boolean
  summaryTitle: string
  summaryWordCount: number
  summaryTotalMs: number
  recentOrder?: number
}

export interface EmailPayload<T> {
  emailId: string
  value: T
}

export interface TabPayload<T> {
  url: string
  value: T
}

export type TabSummaryPayload = TabPayload<AppPageSummary | null> & {
  extra: Record<string, unknown>
}

export const EMAIL_STORES = {
  header: 'email_sessions',
  summary: 'email_summaries',
  replies: 'email_replies',
  chat: 'email_chats',
  metadata: 'email_metadata',
  extras: 'email_extras'
} as const

export const TAB_STORES = {
  header: 'tab_sessions',
  summary: 'tab_summaries',
  chat: 'tab_chats'
} as const

export const EMAIL_STORE_NAMES = Object.values(EMAIL_STORES)
export const TAB_STORE_NAMES = Object.values(TAB_STORES)
export const SESSION_STORE_NAMES = [...EMAIL_STORE_NAMES, ...TAB_STORE_NAMES]
// Negative time keeps newest rows first without reversing primary keys on ties.
export const RECENT_INDEX = 'recent'
export const CACHE_STORE_CONFIGS = [
  ...EMAIL_STORE_NAMES.map(name => ({ name, keyPath: 'emailId',
    ...(name === EMAIL_STORES.header && { indexes: [{ name: RECENT_INDEX, keyPath: 'recentOrder' }] }) })),
  ...TAB_STORE_NAMES.map(name => ({ name, keyPath: 'url',
    ...(name === TAB_STORES.header && { indexes: [{ name: RECENT_INDEX, keyPath: 'recentOrder' }] }) })),
  { name: 'page_cache', keyPath: 'url' },
  { name: 'rag_metadata', keyPath: 'sourceId' }
] as const

const DEFAULT_TTL = 7 * 24 * 60 * 60 * 1000

export function request<T>(idbRequest: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    idbRequest.onsuccess = () => resolve(idbRequest.result)
    idbRequest.onerror = () => reject(idbRequest.error)
  })
}

function putOrDelete(store: IDBObjectStore, key: string, value: unknown): void {
  if (value == null) store.delete(key)
  else store.put(value)
}

function emailHeader(session: CachedEmailSession): EmailHeader {
  const chatCount = session.chatMessages?.length || 0
  const hasSummary = Boolean(session.summary)
  return {
    emailId: session.emailId,
    timestamp: session.timestamp,
    ttl: session.ttl,
    ...(session.emailCount !== undefined && { emailCount: session.emailCount }),
    chatCount,
    hasSummary,
    recentOrder: hasSummary || chatCount > 0 ? -session.timestamp : undefined,
    summaryTitle: session.summary?.summary?.slice(0, 80) || '',
    summaryTimeMs: session.summary?.time_ms || 0
  }
}

function tabHeader(session: TabSessionEntry): TabHeader {
  const chatCount = session.chatMessages?.length || 0
  const hasSummary = Boolean(session.pageSummary)
  return {
    url: session.url,
    timestamp: session.timestamp,
    summaryCollapsed: session.summaryCollapsed,
    chatCount,
    hasSummary,
    recentOrder: hasSummary || chatCount > 0 ? -session.timestamp : undefined,
    summaryTitle: session.pageSummary?.title || '',
    summaryWordCount: session.pageSummary?.wordCount || 0,
    summaryTotalMs: session.pageSummary?.timing?.total || 0
  }
}

function emailExtras(session: CachedEmailSession): Record<string, unknown> {
  const { emailId: _emailId, timestamp: _timestamp, ttl: _ttl, emailCount: _emailCount,
    summary: _summary, replySuggestions: _replySuggestions, chatMessages: _chatMessages,
    metadata: _metadata, ...extra } = session
  return extra
}

function tabExtras(session: TabSessionEntry): Record<string, unknown> {
  const { url: _url, timestamp: _timestamp, chatMessages: _chatMessages,
    pageSummary: _pageSummary, summaryCollapsed: _summaryCollapsed, ...extra } = session
  return extra
}

function writeEmailPayloads(transaction: IDBTransaction, session: CachedEmailSession): void {
  const id = session.emailId
  putOrDelete(transaction.objectStore(EMAIL_STORES.summary), id, session.summary && { emailId: id, value: session.summary })
  putOrDelete(transaction.objectStore(EMAIL_STORES.replies), id, session.replySuggestions == null ? null : { emailId: id, value: session.replySuggestions })
  putOrDelete(transaction.objectStore(EMAIL_STORES.chat), id, session.chatMessages?.length ? { emailId: id, value: session.chatMessages } : null)
  const metadata = session.metadata
  const extra = emailExtras(session)
  putOrDelete(transaction.objectStore(EMAIL_STORES.metadata), id,
    metadata == null ? null : { emailId: id, value: metadata })
  putOrDelete(transaction.objectStore(EMAIL_STORES.extras), id,
    Object.keys(extra).length ? { emailId: id, value: extra } : null)
}

export function writeEmailSession(transaction: IDBTransaction, session: CachedEmailSession): void {
  transaction.objectStore(EMAIL_STORES.header).put(emailHeader(session))
  writeEmailPayloads(transaction, session)
}

export function writeTabSession(transaction: IDBTransaction, session: TabSessionEntry): void {
  transaction.objectStore(TAB_STORES.header).put(tabHeader(session))
  putOrDelete(transaction.objectStore(TAB_STORES.chat), session.url,
    session.chatMessages?.length ? { url: session.url, value: session.chatMessages } : null)
  const extra = tabExtras(session)
  putOrDelete(transaction.objectStore(TAB_STORES.summary), session.url,
    session.pageSummary || Object.keys(extra).length ? {
      url: session.url,
      value: session.pageSummary ?? null,
      extra
    } : null)
}

export async function writeEmailUpdate(
  transaction: IDBTransaction,
  emailId: string,
  updates: Partial<Omit<CachedEmailSession, 'emailId'>>
): Promise<boolean> {
  const before = await request<EmailHeader | undefined>(transaction.objectStore(EMAIL_STORES.header).get(emailId))
  const old = before ?? {
    emailId, timestamp: 0, ttl: DEFAULT_TTL, chatCount: 0,
    hasSummary: false, summaryTitle: '', summaryTimeMs: 0
  }
  const timestamp = updates.timestamp === undefined ? Date.now() : updates.timestamp
  const header: EmailHeader = {
    emailId,
    timestamp,
    ttl: updates.ttl === undefined ? (before?.ttl ?? DEFAULT_TTL) : updates.ttl,
    chatCount: old.chatCount,
    hasSummary: old.hasSummary,
    summaryTitle: old.summaryTitle,
    summaryTimeMs: old.summaryTimeMs,
    ...(old.emailCount !== undefined && { emailCount: old.emailCount }),
    ...(updates.emailCount !== undefined && { emailCount: updates.emailCount })
  }
  const firstSummary = Boolean(updates.summary) && !old.hasSummary
  if (Object.hasOwn(updates, 'summary')) {
    header.hasSummary = Boolean(updates.summary)
    header.summaryTitle = updates.summary?.summary?.slice(0, 80) || ''
    header.summaryTimeMs = updates.summary?.time_ms || 0
    putOrDelete(transaction.objectStore(EMAIL_STORES.summary), emailId,
      updates.summary && { emailId, value: updates.summary })
  }
  if (Object.hasOwn(updates, 'replySuggestions')) {
    putOrDelete(transaction.objectStore(EMAIL_STORES.replies), emailId,
      updates.replySuggestions == null ? null : { emailId, value: updates.replySuggestions })
  }
  if (Object.hasOwn(updates, 'chatMessages')) {
    header.chatCount = updates.chatMessages?.length || 0
    putOrDelete(transaction.objectStore(EMAIL_STORES.chat), emailId,
      updates.chatMessages?.length ? { emailId, value: updates.chatMessages } : null)
  }
  if (Object.hasOwn(updates, 'metadata')) {
    putOrDelete(transaction.objectStore(EMAIL_STORES.metadata), emailId,
      updates.metadata == null ? null : { emailId, value: updates.metadata })
  }
  header.recentOrder = header.hasSummary || header.chatCount > 0 ? -timestamp : undefined
  transaction.objectStore(EMAIL_STORES.header).put(header)
  return firstSummary
}

function assertEmailRow(row: CachedEmailSession): void {
  if (typeof row.emailId !== 'string' || typeof row.timestamp !== 'number' || typeof row.ttl !== 'number') {
    throw new TypeError('Invalid email cache row')
  }
}

function assertTabRow(row: TabSessionEntry): void {
  if (typeof row.url !== 'string' || typeof row.timestamp !== 'number' || typeof row.summaryCollapsed !== 'boolean') {
    throw new TypeError('Invalid tab cache row')
  }
}

function migrateCursor<T>(transaction: IDBTransaction, storeName: string, migrate: (row: T, key: IDBValidKey) => void): void {
  const request = transaction.objectStore(storeName).openCursor()
  request.onsuccess = () => {
    const cursor = request.result
    if (!cursor) return
    try {
      migrate(cursor.value, cursor.primaryKey)
      cursor.continue()
    } catch {
      try { transaction.abort() } catch { /* already aborted */ }
    }
  }
}

export function migrateCache(transaction: IDBTransaction): void {
  try {
    migrateCursor<CachedEmailSession>(transaction, EMAIL_STORES.header, (row, key) => {
      assertEmailRow(row)
      if (key !== row.emailId) throw new TypeError('Mismatched email cache key')
      writeEmailSession(transaction, row)
    })
    migrateCursor<TabSessionEntry>(transaction, TAB_STORES.header, (row, key) => {
      assertTabRow(row)
      if (key !== row.url) throw new TypeError('Mismatched tab cache key')
      writeTabSession(transaction, row)
    })
  } catch {
    try { transaction.abort() } catch { /* already aborted */ }
  }
}
