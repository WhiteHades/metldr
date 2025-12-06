export class SummaryPrefs {
  static ALLOWLIST = [
    'nytimes.com', 'theguardian.com', 'bbc.com', 'cnn.com', 'reuters.com', 'apnews.com', 'npr.org', 'wsj.com',
    'arstechnica.com', 'techcrunch.com', 'theverge.com', 'wired.com', 'engadget.com', 'thenextweb.com',
    'medium.com', 'substack.com', 'dev.to', 'hashnode.com', 'blog.giganticlist.com',
    'docs.', 'documentation.', 'developer.', 'api.', 'guide.', 'docs.github.com', 'developer.mozilla.org',
    'stackoverflow.com', '.edu', 'arxiv.org', 'scholar.google.com', 'wikipedia.org', 'wikimedia.org',
    'news.ycombinator.com', 'reddit.com'
  ];

  static DENYLIST = [
    'mail.google.com', 'gmail.com', 'calendar.google.com', 'drive.google.com',
    'app.slack.com', 'web.whatsapp.com', 'trello.com', 'asana.com', 'notion.so',
    'amazon.com', 'ebay.com', 'etsy.com', 'shopify.', 'cart', 'checkout',
    'twitter.com', 'x.com', 'facebook.com', 'instagram.com', 'tiktok.com', 'linkedin.com', 'youtube.com',
    'search?', '/search', 'results?', 'dashboard', 'admin', 'console', 'panel', 'localhost'
  ];

  static DEFAULT_PREFS = {
    mode: 'manual',
    allowlist: SummaryPrefs.ALLOWLIST,
    denylist: SummaryPrefs.DENYLIST,
    minAutoWords: 500,
    minPromptWords: 250
  };

  static normalizeList(list, fallback) {
    if (!Array.isArray(list)) return fallback;
    return list.map(item => (item || '').toString().trim()).filter(Boolean);
  }

  static parseListInput(text, fallback) {
    if (!text) return [...fallback];
    const parts = text
      .split(/[\n,]/)
      .map(item => item.trim())
      .filter(Boolean);
    return parts.length > 0 ? parts : [...fallback];
  }

  static buildPrefs(stored = {}) {
    return {
      ...SummaryPrefs.DEFAULT_PREFS,
      ...stored,
      allowlist: SummaryPrefs.normalizeList(stored.allowlist, SummaryPrefs.ALLOWLIST),
      denylist: SummaryPrefs.normalizeList(stored.denylist, SummaryPrefs.DENYLIST),
      minAutoWords: Number.isFinite(stored.minAutoWords) ? stored.minAutoWords : SummaryPrefs.DEFAULT_PREFS.minAutoWords,
      minPromptWords: Number.isFinite(stored.minPromptWords) ? stored.minPromptWords : SummaryPrefs.DEFAULT_PREFS.minPromptWords,
    };
  }
}

