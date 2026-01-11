import { databaseService, DB_CONFIGS } from './DatabaseService'
import { logger } from './LoggerService'
import type { 
  EmailSession, 
  EmailSummary, 
  AppChatMessage, 
  AppPageSummary, 
  EmailMetadata 
} from '@/types'
import type { TabSessionEntry } from './CacheService'
import type { DailyStats, SessionRecord, AnalyticsEvent, EventType } from './AnalyticsService'

const log = logger.createScoped('DevSeedService')

// realistic sample data pools
const DOMAINS = [
  'github.com', 'medium.com', 'dev.to', 'stackoverflow.com', 'news.ycombinator.com',
  'arxiv.org', 'reddit.com', 'twitter.com', 'linkedin.com', 'substack.com',
  'notion.so', 'figma.com', 'vercel.com', 'supabase.com', 'planetscale.com',
  'tailwindcss.com', 'react.dev', 'vuejs.org', 'svelte.dev', 'nextjs.org'
]

const PAGE_TITLES = [
  'Understanding React Server Components in 2026',
  'The Complete Guide to TypeScript 6.0 Features',
  'How We Scaled Our Database to 1M Requests/s',
  'Building AI-Powered Applications with Local LLMs',
  'The Future of Web Development: What to Expect',
  'Mastering CSS Container Queries',
  'Why Rust is Taking Over Systems Programming',
  'A Deep Dive into Vector Databases',
  'Optimizing Next.js for Production',
  'The Art of Writing Clean Code',
  'Introduction to Edge Computing',
  'Building Real-time Collaborative Apps',
  'Understanding WebAssembly Performance',
  'The State of JavaScript in 2026',
  'Microservices vs Monoliths: A Practical Guide',
  'Designing Scalable APIs',
  'Machine Learning for Frontend Developers',
  'Privacy-First Architecture Patterns',
  'The Rise of Local-First Software',
  'Building Chrome Extensions with Vue 3'
]

const EMAIL_SUBJECTS = [
  'Your Order #12345 Has Shipped',
  'Meeting Tomorrow at 3pm',
  'Q4 Budget Review - Action Required',
  'Welcome to Our Newsletter',
  'Your Flight Confirmation',
  'Invoice #INV-2026-001',
  'Weekly Team Update',
  'Password Reset Request',
  'Your Subscription Renewal',
  'New Comment on Your Post',
  'Project Deadline Reminder',
  'Contract Review Needed',
  'Travel Expense Report',
  'Performance Review Schedule',
  'System Maintenance Notice'
]

const EMAIL_SENDERS = [
  'John Smith <john@company.com>',
  'Sarah Johnson <sarah@client.org>',
  'Mike Brown <mike@startup.io>',
  'Amazon <ship-confirm@amazon.com>',
  'GitHub <notifications@github.com>',
  'Stripe <receipts@stripe.com>',
  'HR Team <hr@company.com>',
  'Support <support@service.com>',
  'Newsletter <hello@newsletter.com>',
  'Finance <finance@company.com>'
]

const INTENTS = ['informational', 'action_required', 'promotional', 'transactional', 'personal', 'newsletter']
const URGENCIES = ['low', 'medium', 'high']
const MODELS = ['gemini-nano', 'gemini-2.0-flash-exp']

const SUMMARY_BULLETS = [
  'Key points extracted from the document with high accuracy',
  'Main argument presented with supporting evidence',
  'Critical data points and statistics highlighted',
  'Action items identified and prioritized',
  'Technical concepts explained in accessible terms',
  'Historical context provided for better understanding',
  'Future implications discussed with expert analysis',
  'Comparison with alternative approaches included',
  'Best practices and recommendations outlined',
  'Potential risks and mitigation strategies noted'
]

const CHAT_USER_MESSAGES = [
  'Can you explain the main argument in more detail?',
  'What are the key takeaways from this?',
  'How does this compare to the previous version?',
  'Are there any action items I should note?',
  'What are the technical requirements mentioned?',
  'Can you summarize the pricing details?',
  'Who are the main stakeholders involved?',
  'What is the timeline for this project?',
  'Are there any risks mentioned?',
  'What are the next steps?'
]

const CHAT_ASSISTANT_MESSAGES = [
  'Based on the content, the main points are: 1) improved performance metrics, 2) streamlined workflows, and 3) cost reduction strategies.',
  'The key takeaways include enhanced security measures, better user experience, and scalable architecture design.',
  'This version introduces several improvements over the previous iteration, particularly in areas of efficiency and reliability.',
  'There are three main action items: review the proposal, schedule a follow-up meeting, and prepare the implementation plan.',
  'The technical requirements include Node.js 20+, TypeScript 5.x, and compatible with major cloud providers.',
  'The pricing structure includes a free tier, professional at $29/month, and enterprise with custom pricing.',
  'Main stakeholders include the engineering team, product management, and executive leadership.',
  'The project timeline spans 12 weeks, with major milestones at weeks 4, 8, and 12.',
  'Key risks identified include timeline delays, resource constraints, and potential integration challenges.',
  'Next steps involve finalizing the requirements document, setting up the development environment, and beginning the sprint planning process.'
]

function generateId(): string {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 11)}`
}

function randomFrom<T>(arr: T[]): T {
  return arr[Math.floor(Math.random() * arr.length)]
}

function randomBetween(min: number, max: number): number {
  return Math.floor(Math.random() * (max - min + 1)) + min
}

function generateEmailSummary(): EmailSummary {
  return {
    summary: `This email discusses ${randomFrom(['project updates', 'financial matters', 'scheduling', 'product announcements', 'team coordination'])}. ${randomFrom(SUMMARY_BULLETS)}`,
    action_items: Math.random() > 0.5 ? [randomFrom(SUMMARY_BULLETS)] : [],
    dates: Math.random() > 0.5 ? [`${randomFrom(['Tomorrow', 'Next week', 'January 15th', 'Q1 2026'])}`] : [],
    key_facts: {
      booking_reference: Math.random() > 0.7 ? `REF-${randomBetween(100000, 999999)}` : null,
      amount: Math.random() > 0.6 ? `$${randomBetween(10, 5000).toFixed(2)}` : null,
      sender_org: Math.random() > 0.5 ? randomFrom(['Acme Corp', 'Tech Inc', 'Global Services', 'Startup LLC']) : null
    },
    intent: randomFrom(INTENTS),
    tags: [randomFrom(['work', 'personal', 'finance', 'travel', 'shopping'])],
    domain: randomFrom(['business', 'ecommerce', 'newsletter', 'support']),
    reasoning: 'Analyzed email structure and content patterns',
    urgency: randomFrom(URGENCIES),
    time_ms: randomBetween(150, 800),
    cached: Math.random() > 0.3,
    model: randomFrom(MODELS),
    confidence: randomFrom(['high', 'medium', 'low']) as 'high' | 'medium' | 'low'
  }
}

function generatePageSummary(title: string): AppPageSummary {
  const wordCount = randomBetween(500, 5000)
  return {
    title,
    author: Math.random() > 0.3 ? randomFrom(['John Doe', 'Jane Smith', 'Alex Johnson', 'Sam Wilson']) : undefined,
    publishDate: new Date(Date.now() - randomBetween(0, 365) * 24 * 60 * 60 * 1000).toISOString().split('T')[0],
    publication: Math.random() > 0.5 ? randomFrom(DOMAINS) : undefined,
    bullets: Array.from({ length: randomBetween(3, 6) }, () => randomFrom(SUMMARY_BULLETS)),
    readTime: `${Math.ceil(wordCount / 200)} min read`,
    content: 'Full article content would be stored here...',
    wordCount,
    timestamp: Date.now(),
    timing: {
      extraction: randomBetween(50, 200),
      llm: randomBetween(200, 600),
      total: randomBetween(300, 900),
      cached: Math.random() > 0.3,
      model: randomFrom(MODELS),
      provider: randomFrom(['chrome-ai', 'ollama']) as 'chrome-ai' | 'ollama'
    }
  }
}

function generateChatMessages(count: number): AppChatMessage[] {
  const messages: AppChatMessage[] = []
  for (let i = 0; i < count; i++) {
    const isUser = i % 2 === 0
    messages.push({
      role: isUser ? 'user' : 'assistant',
      content: isUser ? randomFrom(CHAT_USER_MESSAGES) : randomFrom(CHAT_ASSISTANT_MESSAGES),
      timing: isUser ? undefined : {
        total: randomBetween(200, 800),
        model: randomFrom(MODELS),
        rag: Math.random() > 0.5 ? randomBetween(50, 150) : undefined,
        embed: Math.random() > 0.5 ? randomBetween(20, 80) : undefined,
        search: Math.random() > 0.5 ? randomBetween(10, 50) : undefined,
        llm: randomBetween(150, 600)
      }
    })
  }
  return messages
}

function generateDailyStats(dateKey: string, dayIndex: number, totalDays: number): DailyStats {
  // create realistic usage patterns
  const baseActivity = 5 + Math.sin(dayIndex / 4) * 3 + Math.random() * 4
  const isWeekend = new Date(dateKey).getDay() === 0 || new Date(dateKey).getDay() === 6
  const activityMultiplier = isWeekend ? 0.6 : 1

  const summaries = Math.floor((baseActivity + randomBetween(2, 8)) * activityMultiplier)
  const chatMessages = Math.floor((baseActivity * 1.5 + randomBetween(3, 12)) * activityMultiplier)
  const panelTime = Math.floor((randomBetween(1200, 3600) + baseActivity * 100) * activityMultiplier)

  // hourly activity with realistic pattern (more activity during work hours)
  const hourlyActivity = Array.from({ length: 24 }, (_, h) => {
    const workHours = h >= 9 && h <= 18
    const peakHours = h >= 10 && h <= 16
    const base = workHours ? (peakHours ? 200 : 100) : 20
    return Math.floor(base * activityMultiplier * (0.5 + Math.random()))
  })

  // domain tracking
  const topDomains: Record<string, number> = {}
  const domainCount = randomBetween(3, 8)
  for (let i = 0; i < domainCount; i++) {
    topDomains[randomFrom(DOMAINS)] = randomBetween(1, 15)
  }

  return {
    date: dateKey,
    timeInPanel: panelTime,
    timeInGmail: Math.floor(panelTime * 0.3),
    timeInArticles: Math.floor(panelTime * 0.5),
    timeInSettings: randomBetween(30, 180),
    timeInStats: randomBetween(60, 300),
    hourlyActivity,
    summariesGenerated: summaries,
    chatMessagesSent: chatMessages,
    chatMessagesReceived: chatMessages,
    wordsLookedUp: randomBetween(0, 8),
    replySuggestionsGenerated: randomBetween(0, 5),
    replySuggestionsUsed: randomBetween(0, 3),
    tokensInput: Math.floor((summaries * 800 + chatMessages * 150) * activityMultiplier),
    tokensOutput: Math.floor((summaries * 200 + chatMessages * 100) * activityMultiplier),
    emailsProcessed: Math.floor(summaries * 0.4),
    pagesProcessed: Math.floor(summaries * 0.5),
    pdfsProcessed: randomBetween(0, 3),
    totalWordsRead: Math.floor((randomBetween(3000, 12000)) * activityMultiplier),
    totalWordsSummarized: Math.floor((randomBetween(400, 1500)) * activityMultiplier),
    avgResponseTimeMs: randomBetween(250, 600),
    responseTimeSamples: summaries + chatMessages,
    fastestResponseMs: randomBetween(120, 200),
    slowestResponseMs: randomBetween(800, 1500),
    topDomains,
    modelUsage: { 'gemini-nano': summaries + chatMessages - 2, 'gemini-2.0-flash-exp': 2 },
    errorCount: Math.random() > 0.9 ? 1 : 0,
    usedChat: true,
    usedSummary: true,
    usedWordLookup: Math.random() > 0.5
  }
}

function generateSessionRecord(timestamp: number): SessionRecord {
  const duration = randomBetween(60, 1800)
  return {
    id: generateId(),
    startTime: timestamp,
    endTime: timestamp + duration * 1000,
    duration,
    context: randomFrom(['gmail', 'article', 'other']) as 'gmail' | 'article' | 'other',
    tab: randomFrom(['summary', 'stats', 'settings']) as 'summary' | 'stats' | 'settings',
    url: `https://${randomFrom(DOMAINS)}/${generateId().slice(0, 8)}`,
    domain: randomFrom(DOMAINS),
    title: randomFrom(PAGE_TITLES),
    summaryGenerated: Math.random() > 0.3,
    chatMessagesCount: randomBetween(0, 8),
    wordsLookedUp: randomBetween(0, 3),
    tokensUsed: randomBetween(500, 2000)
  }
}

function generateEvent(timestamp: number, type: EventType): AnalyticsEvent {
  return {
    id: generateId(),
    timestamp,
    type,
    context: randomFrom(['gmail', 'article', 'other']),
    url: `https://${randomFrom(DOMAINS)}`,
    domain: randomFrom(DOMAINS),
    metadata: {
      model: randomFrom(MODELS),
      responseTimeMs: randomBetween(150, 800)
    }
  }
}

class DevSeedServiceClass {
  async seedAllData(days = 60): Promise<{ 
    emailSessions: number
    tabSessions: number
    dailyStats: number
    sessions: number
    events: number
  }> {
    log.log(`🌱 seeding ${days} days of realistic usage data...`)
    const stats = {
      emailSessions: 0,
      tabSessions: 0,
      dailyStats: 0,
      sessions: 0,
      events: 0
    }

    // seed email sessions
    const emailSessions: EmailSession[] = []
    for (let i = 0; i < days * 2; i++) {
      const daysAgo = Math.floor(i / 2)
      const timestamp = Date.now() - daysAgo * 24 * 60 * 60 * 1000 - randomBetween(0, 12) * 60 * 60 * 1000
      
      const session: EmailSession = {
        emailId: generateId(),
        summary: generateEmailSummary(),
        replySuggestions: null,
        chatMessages: generateChatMessages(randomBetween(0, 6)),
        metadata: {
          from: randomFrom(EMAIL_SENDERS),
          subject: randomFrom(EMAIL_SUBJECTS),
          date: new Date(timestamp).toISOString()
        } as EmailMetadata,
        timestamp,
        ttl: 7 * 24 * 60 * 60 * 1000
      }
      emailSessions.push(session)
    }
    await databaseService.putBatch(DB_CONFIGS.cache, 'email_sessions', emailSessions)
    stats.emailSessions = emailSessions.length
    log.log(`✓ seeded ${emailSessions.length} email sessions`)

    // seed tab sessions
    const tabSessions: TabSessionEntry[] = []
    for (let i = 0; i < days * 3; i++) {
      const daysAgo = Math.floor(i / 3)
      const timestamp = Date.now() - daysAgo * 24 * 60 * 60 * 1000 - randomBetween(0, 12) * 60 * 60 * 1000
      const domain = randomFrom(DOMAINS)
      const title = randomFrom(PAGE_TITLES)
      
      const session: TabSessionEntry = {
        url: `https://${domain}/article/${generateId().slice(0, 8)}`,
        timestamp,
        chatMessages: generateChatMessages(randomBetween(0, 8)),
        pageSummary: generatePageSummary(title),
        summaryCollapsed: Math.random() > 0.3
      }
      tabSessions.push(session)
    }
    await databaseService.putBatch(DB_CONFIGS.cache, 'tab_sessions', tabSessions)
    stats.tabSessions = tabSessions.length
    log.log(`✓ seeded ${tabSessions.length} tab sessions`)

    // seed daily stats
    const dailyStatsArr: DailyStats[] = []
    for (let i = 0; i < days; i++) {
      const date = new Date()
      date.setDate(date.getDate() - (days - i - 1))
      const dateKey = date.toISOString().split('T')[0]
      
      // skip some random days for realistic gaps
      if (Math.random() > 0.85) continue
      
      dailyStatsArr.push(generateDailyStats(dateKey, i, days))
    }
    await databaseService.putBatch(DB_CONFIGS.analytics, 'daily_stats', dailyStatsArr)
    stats.dailyStats = dailyStatsArr.length
    log.log(`✓ seeded ${dailyStatsArr.length} daily stats`)

    // seed session records
    const sessionRecords: SessionRecord[] = []
    for (let i = 0; i < days * 4; i++) {
      const daysAgo = Math.floor(i / 4)
      const timestamp = Date.now() - daysAgo * 24 * 60 * 60 * 1000 - randomBetween(0, 12) * 60 * 60 * 1000
      sessionRecords.push(generateSessionRecord(timestamp))
    }
    await databaseService.putBatch(DB_CONFIGS.analytics, 'sessions', sessionRecords)
    stats.sessions = sessionRecords.length
    log.log(`✓ seeded ${sessionRecords.length} session records`)

    // seed events
    const eventTypes: EventType[] = [
      'summary_completed', 'chat_sent', 'chat_received', 'word_lookup',
      'panel_opened', 'panel_closed', 'tab_switch', 'content_indexed'
    ]
    const events: AnalyticsEvent[] = []
    for (let i = 0; i < days * 15; i++) {
      const daysAgo = Math.floor(i / 15)
      const timestamp = Date.now() - daysAgo * 24 * 60 * 60 * 1000 - randomBetween(0, 12) * 60 * 60 * 1000
      events.push(generateEvent(timestamp, randomFrom(eventTypes)))
    }
    await databaseService.putBatch(DB_CONFIGS.analytics, 'events', events)
    stats.events = events.length
    log.log(`✓ seeded ${events.length} events`)

    log.log('🎉 seeding complete!', stats)
    return stats
  }

  async clearAllSeededData(): Promise<void> {
    log.log('🗑️ clearing all seeded data...')
    
    await Promise.all([
      databaseService.clear(DB_CONFIGS.cache, 'email_sessions'),
      databaseService.clear(DB_CONFIGS.cache, 'tab_sessions'),
      databaseService.clear(DB_CONFIGS.analytics, 'daily_stats'),
      databaseService.clear(DB_CONFIGS.analytics, 'sessions'),
      databaseService.clear(DB_CONFIGS.analytics, 'events')
    ])
    
    log.log('✓ all data cleared')
  }

  async getDataCounts(): Promise<{
    emailSessions: number
    tabSessions: number
    dailyStats: number
    sessions: number
    events: number
  }> {
    const [emailSessions, tabSessions, dailyStats, sessions, events] = await Promise.all([
      databaseService.count(DB_CONFIGS.cache, 'email_sessions'),
      databaseService.count(DB_CONFIGS.cache, 'tab_sessions'),
      databaseService.count(DB_CONFIGS.analytics, 'daily_stats'),
      databaseService.count(DB_CONFIGS.analytics, 'sessions'),
      databaseService.count(DB_CONFIGS.analytics, 'events')
    ])
    return { emailSessions, tabSessions, dailyStats, sessions, events }
  }
}

export const devSeedService = new DevSeedServiceClass()

// expose to global for console access
if (typeof window !== 'undefined') {
  (window as unknown as Record<string, unknown>).devSeedService = devSeedService
}