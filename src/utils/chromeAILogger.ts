/**
 * Chrome AI Debug Logger
 * 
 * comprehensive logging for debugging chrome ai integration
 * easy to enable/disable - just set ENABLED = false to silence
 * 
 * Usage: import { chromeAILogger } from '@/utils/chromeAILogger'
 */

const ENABLED = true;
const PREFIX = '🤖 [Chrome AI]';

// color-coded log levels
const COLORS = {
  info: 'color: #60a5fa; font-weight: bold',
  success: 'color: #4ade80; font-weight: bold',
  warn: 'color: #fbbf24; font-weight: bold',
  error: 'color: #f87171; font-weight: bold',
  debug: 'color: #a78bfa; font-weight: bold',
  api: 'color: #22d3ee; font-weight: bold',
};

function log(level: keyof typeof COLORS, ...args: unknown[]) {
  if (!ENABLED) return;
  console.log(`%c${PREFIX} [${level.toUpperCase()}]`, COLORS[level], ...args);
}

function group(label: string) {
  if (!ENABLED) return;
  console.group(`%c${PREFIX} ${label}`, COLORS.info);
}

function groupEnd() {
  if (!ENABLED) return;
  console.groupEnd();
}

function table(data: unknown) {
  if (!ENABLED) return;
  console.table(data);
}

export const chromeAILogger = {
  // basic logging
  info: (...args: unknown[]) => log('info', ...args),
  success: (...args: unknown[]) => log('success', ...args),
  warn: (...args: unknown[]) => log('warn', ...args),
  error: (...args: unknown[]) => log('error', ...args),
  debug: (...args: unknown[]) => log('debug', ...args),
  api: (...args: unknown[]) => log('api', ...args),
  
  group,
  groupEnd,
  table,
  
  // check browser environment
  checkEnvironment() {
    group('Environment Check');
    
    // browser info
    const ua = navigator.userAgent;
    const chromeMatch = ua.match(/Chrome\/(\d+)/);
    const chromeVersion = chromeMatch ? parseInt(chromeMatch[1]) : null;
    
    log('info', 'User Agent:', ua);
    log('info', 'Chrome Version:', chromeVersion || 'Not Chrome');
    log('info', 'Chrome 138+ Required:', chromeVersion && chromeVersion >= 138 ? '✅ YES' : '❌ NO');
    
    // execution context
    log('info', 'Window object:', typeof window !== 'undefined' ? '✅ Available' : '❌ Missing');
    log('info', 'Is extension context:', typeof chrome !== 'undefined' && chrome.runtime ? '⚠️ YES (may limit APIs)' : 'NO');
    log('info', 'Is service worker:', typeof self !== 'undefined' && 'ServiceWorkerGlobalScope' in self ? '⚠️ YES (APIs unavailable)' : 'NO');
    
    groupEnd();
    return { chromeVersion, isChrome: !!chromeMatch };
  },
  
  // check all chrome ai apis
  async checkAPIs() {
    group('API Availability Check');
    
    const apis = {
      Summarizer: typeof Summarizer !== 'undefined',
      LanguageModel: typeof LanguageModel !== 'undefined',
      Translator: typeof Translator !== 'undefined',
      LanguageDetector: typeof LanguageDetector !== 'undefined',
      Writer: typeof Writer !== 'undefined', 
      Rewriter: typeof Rewriter !== 'undefined',
    };
    
    Object.entries(apis).forEach(([name, available]) => {
      if (available) {
        log('success', `${name}:`, '✅ Available');
      } else {
        log('error', `${name}:`, '❌ Not defined');
      }
    });
    
    // detailed availability check for each api
    const availabilityResults: Record<string, string> = {};
    
    if (apis.Summarizer) {
      try {
        const status = await Summarizer!.availability();
        availabilityResults.Summarizer = status;
        log('api', 'Summarizer.availability():', status);
      } catch (e) {
        availabilityResults.Summarizer = `Error: ${(e as Error).message}`;
        log('error', 'Summarizer.availability() failed:', e);
      }
    }
    
    if (apis.LanguageModel) {
      try {
        const status = await LanguageModel!.availability({ languages: ['en', 'es', 'ja'] });
        availabilityResults.LanguageModel = JSON.stringify(status);
        log('api', 'LanguageModel.availability():', status);
      } catch (e) {
        availabilityResults.LanguageModel = `Error: ${(e as Error).message}`;
        log('error', 'LanguageModel.availability() failed:', e);
      }
    }
    
    if (apis.LanguageDetector) {
      try {
        const status = await LanguageDetector!.availability();
        availabilityResults.LanguageDetector = status;
        log('api', 'LanguageDetector.availability():', status);
      } catch (e) {
        availabilityResults.LanguageDetector = `Error: ${(e as Error).message}`;
        log('error', 'LanguageDetector.availability() failed:', e);
      }
    }
    
    if (apis.Translator) {
      try {
        // translator needs language pair
        const status = await Translator!.availability({ sourceLanguage: 'en', targetLanguage: 'es' });
        availabilityResults.Translator = status;
        log('api', 'Translator.availability(en→es):', status);
      } catch (e) {
        availabilityResults.Translator = `Error: ${(e as Error).message}`;
        log('error', 'Translator.availability() failed:', e);
      }
    }
    
    if (apis.Writer) {
      try {
        const status = await Writer!.availability();
        availabilityResults.Writer = status;
        log('api', 'Writer.availability():', status);
      } catch (e) {
        availabilityResults.Writer = `Error: ${(e as Error).message}`;
        log('error', 'Writer.availability() failed:', e);
      }
    }
    
    if (apis.Rewriter) {
      try {
        const status = await Rewriter!.availability();
        availabilityResults.Rewriter = status;
        log('api', 'Rewriter.availability():', status);
      } catch (e) {
        availabilityResults.Rewriter = `Error: ${(e as Error).message}`;
        log('error', 'Rewriter.availability() failed:', e);
      }
    }
    
    groupEnd();
    return { apis, availabilityResults };
  },
  
  // test summarizer with sample text
  async testSummarizer() {
    group('Summarizer Test');
    
    if (typeof Summarizer === 'undefined') {
      log('error', 'Summarizer API not available');
      groupEnd();
      return { success: false, error: 'API not available' };
    }
    
    try {
      log('info', 'Checking availability...');
      const availability = await Summarizer.availability();
      log('api', 'Availability:', availability);
      
      if (availability === 'unavailable') {
        log('error', 'Summarizer unavailable on this device');
        groupEnd();
        return { success: false, error: 'Unavailable' };
      }
      
      if (availability === 'downloadable') {
        log('warn', 'Model needs to be downloaded first');
        log('info', 'Attempting to trigger download...');
      }
      
      log('info', 'Creating summarizer instance...');
      const startCreate = performance.now();
      
      const summarizer = await Summarizer.create({
        type: 'key-points',
        format: 'plain-text',
        length: 'short',
        expectedInputLanguages: ['en', 'es', 'ja'],
        expectedContextLanguages: ['en'],
        outputLanguage: 'en',
        monitor: (m) => {
          m.addEventListener('downloadprogress', (e) => {
            log('info', `Download progress: ${e.loaded}%`);
          });
        }
      });
      
      log('success', `Summarizer created in ${(performance.now() - startCreate).toFixed(0)}ms`);
      
      const testText = `
        Artificial intelligence is transforming how we interact with technology.
        Machine learning models can now understand natural language, generate images,
        and even write code. These advances have led to new applications in healthcare,
        education, and entertainment. However, there are also concerns about AI safety,
        bias in training data, and the impact on employment.
      `;
      
      log('info', 'Running test summarization...');
      const startSummarize = performance.now();
      const result = await summarizer.summarize(testText);
      const duration = performance.now() - startSummarize;
      
      log('success', `Summarization complete in ${duration.toFixed(0)}ms`);
      log('success', 'Result:', result);
      
      summarizer.destroy();
      log('info', 'Summarizer instance destroyed');
      
      groupEnd();
      return { success: true, result, duration };
      
    } catch (e) {
      log('error', 'Test failed:', e);
      groupEnd();
      return { success: false, error: (e as Error).message };
    }
  },
  
  // test language model (prompt api)
  async testLanguageModel() {
    group('Language Model Test');
    
    if (typeof LanguageModel === 'undefined') {
      log('error', 'LanguageModel API not available');
      groupEnd();
      return { success: false, error: 'API not available' };
    }
    
    try {
      log('info', 'Checking availability...');
      const langOpts = { languages: ['en', 'es', 'ja'] }
      const availability = await LanguageModel.availability(langOpts);
      log('api', 'Availability:', availability);
      
      const status = typeof availability === 'string' ? availability : availability?.available
      if (status === 'no' || status === 'unavailable') {
        log('error', 'LanguageModel unavailable');
        groupEnd();
        return { success: false, error: 'Unavailable' };
      }
      
      log('info', 'Creating session...');
      const startCreate = performance.now();
      
      const session = await LanguageModel.create({
        initialPrompts: [{ role: 'system', content: 'You are a helpful assistant. Be concise.' }],
        expectedInputs: [{ type: 'text', languages: ['en', 'es', 'ja'] }],
        expectedOutputs: [{ type: 'text', languages: ['en'] }],
        monitor: (m) => {
          m.addEventListener('downloadprogress', (e) => {
            log('info', `Download progress: ${e.loaded}%`);
          });
        }
      });
      
      log('success', `Session created in ${(performance.now() - startCreate).toFixed(0)}ms`);
      log('info', 'Token usage:', { input: session.inputUsage, quota: session.inputQuota });
      
      log('info', 'Running test prompt...');
      const startPrompt = performance.now();
      const result = await session.prompt('Say hello in exactly 5 words.');
      const duration = performance.now() - startPrompt;
      
      log('success', `Prompt complete in ${duration.toFixed(0)}ms`);
      log('success', 'Result:', result);
      
      session.destroy();
      log('info', 'Session destroyed');
      
      groupEnd();
      return { success: true, result, duration };
      
    } catch (e) {
      log('error', 'Test failed:', e);
      groupEnd();
      return { success: false, error: (e as Error).message };
    }
  },
  
  // run full diagnostic
  async runDiagnostic() {
    console.clear();
    console.log('%c' + '═'.repeat(60), 'color: #8b5cf6');
    console.log('%c  CHROME AI DIAGNOSTIC REPORT', 'color: #8b5cf6; font-size: 16px; font-weight: bold');
    console.log('%c' + '═'.repeat(60), 'color: #8b5cf6');
    console.log('');
    
    const env = this.checkEnvironment();
    console.log('');
    
    const apis = await this.checkAPIs();
    console.log('');
    
    const summarizerTest = await this.testSummarizer();
    console.log('');
    
    const lmTest = await this.testLanguageModel();
    console.log('');
    
    // summary
    group('DIAGNOSTIC SUMMARY');
    
    const issues: string[] = [];
    
    if (!env.chromeVersion || env.chromeVersion < 138) {
      issues.push('Chrome version too old (need 138+)');
    }
    
    if (!apis.apis.Summarizer) {
      issues.push('Summarizer API not exposed (check chrome://flags)');
    }
    
    if (!apis.apis.LanguageModel) {
      issues.push('LanguageModel API not exposed (check chrome://flags)');
    }
    
    if (apis.availabilityResults.Summarizer === 'unavailable') {
      issues.push('Summarizer marked unavailable (device may not support)');
    }
    
    if (apis.availabilityResults.Summarizer === 'downloadable') {
      issues.push('Model not downloaded yet - trigger download from chrome://components');
    }
    
    if (!summarizerTest.success) {
      issues.push(`Summarizer test failed: ${summarizerTest.error}`);
    }
    
    if (issues.length === 0) {
      log('success', '✅ ALL CHECKS PASSED - Chrome AI is working!');
    } else {
      log('error', '❌ ISSUES FOUND:');
      issues.forEach((issue, i) => {
        log('warn', `  ${i + 1}. ${issue}`);
      });
    }
    
    groupEnd();
    
    console.log('');
    console.log('%c' + '═'.repeat(60), 'color: #8b5cf6');
    console.log('%c  To run again: chromeAILogger.runDiagnostic()', 'color: #71717a');
    console.log('%c' + '═'.repeat(60), 'color: #8b5cf6');
    
    return { env, apis, summarizerTest, lmTest, issues };
  }
};

// expose to window for console access
if (typeof window !== 'undefined') {
  (window as unknown as { chromeAILogger: typeof chromeAILogger }).chromeAILogger = chromeAILogger;
}

export default chromeAILogger;
