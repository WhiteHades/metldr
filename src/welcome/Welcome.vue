<script setup>
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { useThemeStore } from '../stores/theme';
import { getOsType, getSetupCommands } from '../utils/platformUtils';
import { gsap } from 'gsap';
import { 
  Download, Cpu, Sparkles, Check, Copy, 
  ChevronRight, ChevronDown, ExternalLink, Zap, Shield, Globe,
  FileText, MessageCircle, Mail, Reply, BookOpen, Languages, HelpCircle,
  Command, AppWindow, Terminal, Loader2, AlertCircle, CheckCircle2, Cloud, RefreshCw
} from 'lucide-vue-next';
import { Button } from '../components/ui';

const themeStore = useThemeStore();
const detectedOs = ref(getOsType());
const platformSetup = computed(() => getSetupCommands());

const osIcon = computed(() => {
  switch (detectedOs.value) {
    case 'macos': return Command;
    case 'windows': return AppWindow;
    default: return Terminal;
  }
});

const copiedStates = ref({});

// chrome ai state
const chromeAIStatus = ref('checking'); // 'checking' | 'unavailable' | 'no-browser' | 'available' | 'downloadable' | 'downloading' | 'ready'
const chromeAIProgress = ref(0);
const chromeAIMessage = ref('checking chrome ai availability...');
const chromeVersion = ref('');
let statusPollingInterval = null;

// check chrome ai availability
async function checkChromeAI() {
  try {
    // check browser version
    const ua = navigator.userAgent;
    const chromeMatch = ua.match(/Chrome\/(\d+)/);
    if (chromeMatch) {
      chromeVersion.value = chromeMatch[1];
    }
    
    // check if summarizer api exists
    if (typeof Summarizer === 'undefined') {
      chromeAIStatus.value = 'no-browser';
      chromeAIMessage.value = `chrome ai requires chrome 138+ (you have ${chromeVersion.value || 'unknown'})`;
      return;
    }
    
    const availability = await Summarizer.availability();
    
    switch (availability) {
      case 'available':
        chromeAIStatus.value = 'ready';
        chromeAIMessage.value = 'gemini nano is ready to use!';
        chromeAIProgress.value = 100;
        break;
      case 'downloadable':
        chromeAIStatus.value = 'downloadable';
        chromeAIMessage.value = 'gemini nano can be downloaded (~1.5GB)';
        chromeAIProgress.value = 0;
        break;
      case 'downloading':
        chromeAIStatus.value = 'downloading';
        chromeAIMessage.value = 'gemini nano is downloading...';
        startProgressPolling();
        break;
      case 'unavailable':
        chromeAIStatus.value = 'unavailable';
        chromeAIMessage.value = 'gemini nano is not available on this device';
        break;
      default:
        chromeAIStatus.value = 'unavailable';
        chromeAIMessage.value = 'could not determine chrome ai status';
    }
  } catch (err) {
    console.error('[Welcome] Chrome AI check failed:', err);
    chromeAIStatus.value = 'no-browser';
    chromeAIMessage.value = 'chrome ai not available in this browser';
  }
}

// trigger model download by creating a summarizer instance
async function triggerChromeAIDownload() {
  if (chromeAIStatus.value !== 'downloadable') return;
  
  chromeAIStatus.value = 'downloading';
  chromeAIMessage.value = 'starting gemini nano download...';
  chromeAIProgress.value = 0;
  
  try {
    // creating a summarizer instance triggers the download
    await Summarizer.create({
      type: 'key-points',
      format: 'plain-text',
      length: 'medium',
      expectedInputLanguages: ['en', 'es', 'ja'],
      expectedContextLanguages: ['en'],
      outputLanguage: 'en',
      monitor: (m) => {
        m.addEventListener('downloadprogress', (e) => {
          if (e.loaded) {
            chromeAIProgress.value = Math.min(e.loaded, 100);
            chromeAIMessage.value = `downloading gemini nano... ${Math.round(e.loaded)}%`;
          }
        });
      }
    });
    
    chromeAIStatus.value = 'ready';
    chromeAIMessage.value = 'gemini nano is ready to use!';
    chromeAIProgress.value = 100;
    stopProgressPolling();
  } catch (err) {
    console.error('[Welcome] Chrome AI download failed:', err);
    chromeAIStatus.value = 'downloadable';
    chromeAIMessage.value = 'download failed. click to retry.';
  }
}

// poll for download progress (fallback when we can't track directly)
function startProgressPolling() {
  stopProgressPolling();
  statusPollingInterval = setInterval(async () => {
    try {
      const availability = await Summarizer.availability();
      if (availability === 'available') {
        chromeAIStatus.value = 'ready';
        chromeAIMessage.value = 'gemini nano is ready to use!';
        chromeAIProgress.value = 100;
        stopProgressPolling();
      } else if (availability === 'downloading') {
        // simulate progress during download
        if (chromeAIProgress.value < 95) {
          chromeAIProgress.value += Math.random() * 3;
        }
      }
    } catch (err) {
      // ignore polling errors
    }
  }, 2000);
}

function stopProgressPolling() {
  if (statusPollingInterval) {
    clearInterval(statusPollingInterval);
    statusPollingInterval = null;
  }
}

function openChromeFlags() {
  // can't directly open chrome:// URLs, so copy to clipboard
  navigator.clipboard.writeText('chrome://flags/#optimization-guide-on-device-model');
  copiedStates.value['chrome-flags'] = true;
  setTimeout(() => { copiedStates.value['chrome-flags'] = false; }, 2500);
}

const features = [
  {
    icon: FileText,
    title: 'page summaries',
    desc: 'get instant bullet-point summaries of any article or webpage',
    color: 'violet'
  },
  {
    icon: MessageCircle,
    title: 'contextual chat',
    desc: 'ask questions about the page content and get smart answers',
    color: 'cyan'
  },
  {
    icon: Mail,
    title: 'email summaries',
    desc: 'auto-summarize gmail threads with action items and key dates',
    color: 'rose'
  },
  {
    icon: Reply,
    title: 'smart replies',
    desc: 'ai-generated reply suggestions that match your tone',
    color: 'amber'
  },
  {
    icon: BookOpen,
    title: 'word lookup',
    desc: 'select any word to see definitions, synonyms, and translations',
    color: 'emerald'
  },
  {
    icon: Languages,
    title: 'multi-language',
    desc: 'offline dictionaries for english, spanish, french, german & more',
    color: 'blue'
  }
];

const recommendedModels = [
  { name: 'llama3.2:1b', size: '1b', desc: 'recommended default', speed: 'fast_' },
  { name: 'gemma3:1b', size: '1b', desc: 'ultra-fast, lightweight', speed: 'fast_' },
  { name: 'qwen3:1.4b', size: '1.4b', desc: 'compact & capable', speed: 'fast_' },
  { name: 'gemma3n:e2b', size: '2b', desc: 'efficient edge model', speed: 'fast' },
  { name: 'deepseek-r1:1.5b', size: '1.5b', desc: 'reasoning focused', speed: 'fast' },
  { name: 'qwen3:1.7b', size: '1.7b', desc: 'balanced performance', speed: 'fast' },
  { name: 'ministral-3b', size: '3b', desc: 'mistral\'s mini model', speed: 'fast' },
  { name: 'llama3.2:3b', size: '3b', desc: 'great for most tasks', speed: 'moderate' },
  { name: 'gemma3:4b', size: '4b', desc: 'google\'s quality model', speed: 'moderate' },
  { name: 'gemma3n:e4b', size: '4b', desc: 'edge optimised', speed: 'moderate' },
];

const heroRef = ref(null);
const featuresRef = ref(null);
const stepsRef = ref(null);
const modelsRef = ref(null);
const ctaRef = ref(null);
const faqRef = ref(null);
const expandedFaq = ref(null);

const faqs = [
  {
    question: 'what is gemini nano and do i need it?',
    answer: "gemini nano is google's on-device ai model built into chrome 138+. it's the recommended option because it requires zero setup - just enable the flags and you're ready. if you have an older browser or prefer running your own models, use ollama instead."
  },
  {
    question: 'chrome ai says "unavailable" - how do i fix it?',
    answer: 'this usually means the flags are not enabled or you need to restart chrome. go to `chrome://flags`, search for "gemini nano" and enable all 4 flags: `#optimization-guide-on-device-model` (set to BypassPerfRequirement), `#prompt-api-for-gemini-nano`, `#summarization-api-for-gemini-nano`, and `#writer-api-for-gemini-nano`. then fully quit and reopen chrome.'
  },
  {
    question: 'gemini nano download is stuck or failed',
    answer: 'the model is ~1.5GB and downloads in the background. if it seems stuck, try: 1) paste `chrome://components` in the address bar, 2) find "Optimization Guide On Device Model", 3) click "Check for update". you can also try disabling and re-enabling the flags, then restarting chrome.'
  },
  {
    question: 'what is ollama and when should i use it?',
    answer: "ollama runs open-source ai models locally on your computer. use it if: you have chrome < 138, gemini nano is not available on your device, or you want to use specific models like llama, mistral, or deepseek. it requires manual setup but gives you more control."
  },
  {
    question: 'why is ollama not connecting?',
    answer: "make sure ollama is running and you've completed the cors setup (step 2 in the ollama section). after setting the environment variable, you need to fully quit and restart ollama. the extension auto-detects ollama every 2 seconds."
  },
  {
    question: 'is my data sent anywhere?',
    answer: 'no. with both gemini nano and ollama, all processing happens locally on your computer. gemini nano runs inside chrome, ollama runs on localhost. no data is ever sent to external servers - your browsing stays completely private.'
  },
  {
    question: 'how do i summarise gmail emails?',
    answer: 'when viewing an email thread in gmail, you will see a "metldr - summarise email" button. click it to generate a summary with key points, action items, and important dates. you can enable auto-summarisation in the extension settings.'
  },
  {
    question: 'which option is faster?',
    answer: "gemini nano is typically faster for most tasks since it's optimised for chrome. ollama speed depends on your chosen model - 1b-2b models are fast, 3b+ models are slower but smarter. if you have a dedicated gpu, ollama can be very fast."
  }
];

function toggleFaq(index) {
  expandedFaq.value = expandedFaq.value === index ? null : index;
}

function formatFaqText(text) {
  // convert backticks to styled code
  let formatted = text.replace(/`([^`]+)`/g, '<code class="inline-code">$1</code>');
  // convert button text to styled badge
  formatted = formatted.replace(
    /"metldr - summarise email"/g,
    '<span class="button-badge">metldr - summarise email</span>'
  );
  return formatted;
}

function copyToClipboard(text, key) {
  navigator.clipboard.writeText(text).then(() => {
    copiedStates.value[key] = true;
    setTimeout(() => {
      copiedStates.value[key] = false;
    }, 2000);
  });
}

function getStarted() {
  chrome.storage.local.set({ onboardingComplete: true }).then(() => {
    window.close();
  });
}

function openOllamaWebsite() {
  window.open('https://ollama.com', '_blank');
}

onMounted(async () => {
  await themeStore.loadSavedTheme();
  
  // check chrome ai availability
  checkChromeAI();
  
  // wait for fonts to load to prevent jank
  if (document.fonts && document.fonts.ready) {
    await document.fonts.ready;
  }
  
  // reveal app container
  const appEl = document.getElementById('welcome-app');
  if (appEl) appEl.classList.add('ready');
  
  // small delay for browser paint
  await new Promise(r => requestAnimationFrame(r));
  
  // smooth fade-in animations
  gsap.fromTo(heroRef.value, 
    { opacity: 0, y: 30 }, 
    { opacity: 1, y: 0, duration: 0.8, ease: 'power3.out' }
  );
  
  gsap.fromTo('.feature-card', 
    { opacity: 0, y: 20 }, 
    { opacity: 1, y: 0, duration: 0.5, stagger: 0.06, ease: 'power2.out', delay: 0.2 }
  );
  
  gsap.fromTo('.step', 
    { opacity: 0, x: -20 }, 
    { opacity: 1, x: 0, duration: 0.5, stagger: 0.1, ease: 'power2.out', delay: 0.4 }
  );
  
  gsap.fromTo('.model-card', 
    { opacity: 0, y: 15 }, 
    { opacity: 1, y: 0, duration: 0.4, stagger: 0.04, ease: 'power2.out', delay: 0.6 }
  );
  
  gsap.fromTo(ctaRef.value, 
    { opacity: 0, scale: 0.95 }, 
    { opacity: 1, scale: 1, duration: 0.5, ease: 'back.out(1.5)', delay: 0.8 }
  );

  gsap.fromTo('.faq-item', 
    { opacity: 0, y: 15 }, 
    { opacity: 1, y: 0, duration: 0.4, stagger: 0.08, ease: 'power2.out', delay: 1.0 }
  );

  gsap.to('.float-shape', {
    y: -15,
    duration: 2.5,
    ease: 'power1.inOut',
    yoyo: true,
    repeat: -1,
    stagger: 0.3
  });
  
  gsap.to('.pulse-ring', {
    scale: 1.5,
    opacity: 0,
    duration: 2,
    ease: 'power2.out',
    repeat: -1
  });
});

onUnmounted(() => {
  stopProgressPolling();
});
</script>

<template>
  <div class="welcome-page">
    <div class="bg-pattern"></div>
    
    <!-- floating shapes -->
    <div class="shapes-container">
      <div class="float-shape shape-1"></div>
      <div class="float-shape shape-2"></div>
      <div class="float-shape shape-3"></div>
    </div>
    
    <div class="content">
      <!-- hero -->
      <section ref="heroRef" class="hero">
        <div class="hero-glow"></div>
        
        <div class="badge">
          <div class="pulse-ring"></div>
          <span>private & local ai</span>
        </div>
        
        <h1 class="title">
          welcome to
          <span class="gradient-text">metldr</span>
        </h1>
        
        <p class="subtitle">
          free & open source ai assistant that runs entirely on your machine. 
          <strong>no cloud, no tracking, just you and your ai.</strong>
        </p>
        
        <div class="hero-features">
          <div class="hero-feature">
            <Shield :size="18" />
            <span>100% private</span>
          </div>
          <div class="hero-feature">
            <Zap :size="18" />
            <span>runs locally</span>
          </div>
          <div class="hero-feature">
            <Globe :size="18" />
            <span>works offline</span>
          </div>
        </div>
      </section>
      
      <!-- features grid -->
      <section ref="featuresRef" class="features-section">
        <h2 class="section-title">what you can do</h2>
        
        <div class="features-grid">
          <div 
            v-for="(feature, index) in features" 
            :key="feature.title"
            class="feature-card"
            :class="`feature-${feature.color}`"
          >
            <div class="feature-icon" :class="`icon-${feature.color}`">
              <component :is="feature.icon" :size="24" />
            </div>
            <div class="feature-content">
              <h3>{{ feature.title }}</h3>
              <p>{{ feature.desc }}</p>
            </div>
            <div class="feature-bg"></div>
          </div>
        </div>
      </section>
      
      <!-- OPTION 1: gemini nano (recommended) -->
      <section class="setup-section">
        <div class="setup-header">
          <div class="setup-badge recommended">
            <span>recommended</span>
          </div>
          <h2 class="section-title">option 1: gemini nano</h2>
          <p class="setup-desc">zero setup — runs directly in chrome with no downloads or configuration</p>
        </div>
        
        <!-- live status -->
        <div class="gemini-status" :class="chromeAIStatus">
          <div class="gemini-status-icon">
            <Loader2 v-if="chromeAIStatus === 'checking' || chromeAIStatus === 'downloading'" :size="20" class="spinning" />
            <CheckCircle2 v-else-if="chromeAIStatus === 'ready'" :size="20" />
            <Cloud v-else-if="chromeAIStatus === 'downloadable'" :size="20" />
            <AlertCircle v-else :size="20" />
          </div>
          <div class="gemini-status-content">
            <span class="gemini-status-title">
              <template v-if="chromeAIStatus === 'checking'">checking...</template>
              <template v-else-if="chromeAIStatus === 'ready'">ready to use!</template>
              <template v-else-if="chromeAIStatus === 'downloadable'">available for download</template>
              <template v-else-if="chromeAIStatus === 'downloading'">downloading...</template>
              <template v-else>setup required</template>
            </span>
            <span class="gemini-status-msg">{{ chromeAIMessage }}</span>
          </div>
          <div v-if="chromeAIStatus === 'downloading'" class="gemini-progress">
            <div class="gemini-progress-bar">
              <div class="gemini-progress-fill" :style="{ width: chromeAIProgress + '%' }"></div>
            </div>
            <span>{{ Math.round(chromeAIProgress) }}%</span>
          </div>
          <Button 
            v-if="chromeAIStatus === 'downloadable'"
            @click="triggerChromeAIDownload"
            class="gemini-action-btn"
          >
            <Download :size="14" />
            download
          </Button>
          <Button 
            v-else-if="chromeAIStatus === 'ready'"
            @click="getStarted"
            class="gemini-action-btn ready"
          >
            <Check :size="14" />
            get started
          </Button>
        </div>
        
        <!-- setup steps (only if not ready) -->
        <div v-if="chromeAIStatus !== 'ready'" ref="stepsRef" class="steps">
          <div class="step">
            <div class="step-number">1</div>
            <div class="step-body">
              <div class="step-header">
                <Globe :size="20" />
                <h3>use chrome 138+</h3>
                <span v-if="chromeVersion" class="os-badge">you have {{ chromeVersion }}</span>
              </div>
              <p>gemini nano requires chrome canary or dev channel (138+).</p>
              <div class="command-box">
                <code>chrome://version</code>
                <Button 
                  @click="copyToClipboard('chrome://version', 'chrome-version')"
                  class="copy-btn p-0 h-7 w-7"
                  variant="ghost"
                  :class="{ copied: copiedStates['chrome-version'] }"
                >
                  <Check v-if="copiedStates['chrome-version']" :size="16" />
                  <Copy v-else :size="16" />
                </Button>
              </div>
            </div>
          </div>
          
          <div class="step">
            <div class="step-number">2</div>
            <div class="step-body">
              <div class="step-header">
                <Zap :size="20" />
                <h3>enable ai flags</h3>
              </div>
              <p>paste this url and enable the flags below, then restart chrome.</p>
              <div class="command-box">
                <code>chrome://flags/#optimization-guide-on-device-model</code>
                <Button 
                  @click="copyToClipboard('chrome://flags/#optimization-guide-on-device-model', 'flags-url')"
                  class="copy-btn p-0 h-7 w-7"
                  variant="ghost"
                  :class="{ copied: copiedStates['flags-url'] }"
                >
                  <Check v-if="copiedStates['flags-url']" :size="16" />
                  <Copy v-else :size="16" />
                </Button>
              </div>
              <ul class="flag-list">
                <li><code>#optimization-guide-on-device-model</code> → <strong>Enabled BypassPerfRequirement</strong></li>
                <li><code>#prompt-api-for-gemini-nano</code> → <strong>Enabled</strong></li>
                <li><code>#summarization-api-for-gemini-nano</code> → <strong>Enabled</strong></li>
                <li><code>#writer-api-for-gemini-nano</code> → <strong>Enabled</strong></li>
              </ul>
            </div>
          </div>
          
          <div class="step">
            <div class="step-number">3</div>
            <div class="step-body">
              <div class="step-header">
                <RefreshCw :size="20" />
                <h3>restart & verify</h3>
              </div>
              <p>after enabling flags, fully restart chrome and return here.</p>
              <Button @click="checkChromeAI" variant="outline" class="recheck-btn-inline">
                <Loader2 v-if="chromeAIStatus === 'checking'" :size="14" class="spinning" />
                <Zap v-else :size="14" />
                recheck status
              </Button>
            </div>
          </div>

          <div class="troubleshoot-box">
            <div class="troubleshoot-header">
              <HelpCircle :size="16" />
              <span>requirements & troubleshooting</span>
            </div>
            <ul class="troubleshoot-list">
              <li><strong>storage:</strong> 22GB+ free space on chrome profile drive</li>
              <li><strong>hardware:</strong> 4GB+ VRAM (GPU) or 16GB RAM + 4 CPU cores</li>
              <li><strong>download stuck?</strong> paste <code>chrome://components</code> → find "Optimization Guide On Device Model" → click "Check for update"</li>
              <li><strong>check model status:</strong> paste <code>chrome://on-device-internals</code></li>
            </ul>
          </div>
        </div>
      </section>
      
      <!-- divider -->
      <div class="section-divider">
        <span>or use ollama</span>
      </div>
      
      <!-- OPTION 2: ollama (fallback) -->
      <section class="setup-section ollama-section">
        <div class="setup-header">
          <div class="setup-badge fallback">
            <span>alternative</span>
          </div>
          <h2 class="section-title">option 2: ollama</h2>
          <p class="setup-desc">for older browsers or if you prefer running your own models</p>
        </div>
        
        <div ref="stepsRef" class="steps">
          <!-- step 1 -->
          <div class="step">
            <div class="step-number">1</div>
            <div class="step-body">
              <div class="step-header">
                <Download :size="20" />
                <h3>install ollama</h3>
                <span class="os-badge">{{ platformSetup.os.toLowerCase() }}</span>
              </div>
              <p>ollama runs ai models locally on your machine.</p>
              
              <div class="command-box">
                <code>{{ platformSetup.install }}</code>
                <Button 
                  @click="copyToClipboard(platformSetup.install, 'install')"
                  class="copy-btn p-0 h-7 w-7"
                  variant="ghost"
                  :class="{ copied: copiedStates['install'] }"
                >
                  <Check v-if="copiedStates['install']" :size="16" />
                  <Copy v-else :size="16" />
                </Button>
              </div>
              
              <Button @click="openOllamaWebsite" variant="link" class="link-btn h-auto p-0 font-normal">
                or download from ollama.com
                <ExternalLink :size="12" />
              </Button>
            </div>
          </div>
          
          <!-- step 2 -->
          <div class="step">
            <div class="step-number">2</div>
            <div class="step-body">
              <div class="step-header">
                <component :is="osIcon" :size="20" />
                <h3>enable extension access</h3>
                <span class="os-badge">{{ platformSetup.os.toLowerCase() }}</span>
                <span class="os-badge" style="background: rgba(34, 197, 94, 0.15); color: #4ade80;">one-time setup</span>
              </div>
              <p>{{ platformSetup.permanentSetup.note }}</p>
              
              <!-- windows  -->
              <template v-if="detectedOs === 'windows'">
                <p class="step-subtitle">{{ platformSetup.permanentSetup.commandNote }}</p>
                <div class="command-box">
                  <code>{{ platformSetup.permanentSetup.command }}</code>
                  <Button 
                    @click="copyToClipboard(platformSetup.permanentSetup.command, 'permanent')"
                    class="copy-btn p-0 h-7 w-7"
                    variant="ghost"
                    :class="{ copied: copiedStates['permanent'] }"
                  >
                    <Check v-if="copiedStates['permanent']" :size="16" />
                    <Copy v-else :size="16" />
                  </Button>
                </div>
                <p class="step-note">after running this, right-click ollama in the system tray → quit, then relaunch it.</p>
                
                <details class="alt-method">
                  <summary>
                    <span>alternative: set via windows settings</span>
                  </summary>
                  <ol class="manual-steps">
                    <li v-for="(step, i) in platformSetup.permanentSetup.steps" :key="i">{{ step }}</li>
                  </ol>
                </details>
              </template>
              
              <!-- macOS -->
              <template v-else-if="detectedOs === 'macos'">
                <p class="step-subtitle">{{ platformSetup.permanentSetup.commandNote }}</p>
                <div class="command-box">
                  <code>{{ platformSetup.permanentSetup.command }}</code>
                  <Button 
                    @click="copyToClipboard(platformSetup.permanentSetup.command, 'permanent')"
                    class="copy-btn p-0 h-7 w-7"
                    variant="ghost"
                    :class="{ copied: copiedStates['permanent'] }"
                  >
                    <Check v-if="copiedStates['permanent']" :size="16" />
                    <Copy v-else :size="16" />
                  </Button>
                </div>
                <p class="step-note">then quit ollama from the menu bar (click the llama icon → quit) and relaunch it.</p>
              </template>
              
              <!-- linux -->
              <template v-else>
                <p class="step-subtitle">{{ platformSetup.permanentSetup.commandNote }}</p>
                <div v-for="(cmd, i) in platformSetup.permanentSetup.commands" :key="i" class="command-box" style="margin-bottom: 8px;">
                  <code>{{ cmd }}</code>
                  <Button 
                    @click="copyToClipboard(cmd, `linux-${i}`)"
                    class="copy-btn p-0 h-7 w-7"
                    variant="ghost"
                    :class="{ copied: copiedStates[`linux-${i}`] }"
                  >
                    <Check v-if="copiedStates[`linux-${i}`]" :size="16" />
                    <Copy v-else :size="16" />
                  </Button>
                </div>
              </template>
              
              <details class="alt-method">
                <summary>
                  <span>quick test (temporary, this session only)</span>
                </summary>
                <p class="alt-desc">if you just want to test quickly, run this in a terminal. note: you'll need to run it again each time.</p>
                <div class="command-box">
                  <code>{{ platformSetup.serve }}</code>
                  <Button 
                    @click="copyToClipboard(platformSetup.serve, 'serve')"
                    class="copy-btn p-0 h-7 w-7"
                    variant="ghost"
                    :class="{ copied: copiedStates['serve'] }"
                  >
                    <Check v-if="copiedStates['serve']" :size="16" />
                    <Copy v-else :size="16" />
                  </Button>
                </div>
              </details>
            </div>
          </div>
          
          <!-- step 3 -->
          <div class="step">
            <div class="step-number">3</div>
            <div class="step-body">
              <div class="step-header">
                <Cpu :size="20" />
                <h3>pull a model</h3>
              </div>
              <p>open a new terminal and run one of the commands below. "pulling" downloads the ai model to your computer. smaller models (1b-2b) are faster; larger models (3b-4b) are smarter but slower.</p>
            </div>
          </div>
        </div>
      </section>
      
      <!-- models -->
      <section ref="modelsRef" class="models-section">
        <div class="models-grid">
          <div 
            v-for="model in recommendedModels" 
            :key="model.name"
            class="model-card"
          >
            <div class="model-top">
              <span class="model-name">{{ model.name }}</span>
              <span class="speed-tag" :class="model.speed">{{ model.speed }}</span>
            </div>
            <p class="model-desc">{{ model.desc }}</p>
            <div class="model-cmd">
              <code>ollama pull {{ model.name }}</code>
              <Button 
                @click="copyToClipboard(`ollama pull ${model.name}`, model.name)"
                class="copy-btn-sm p-0 h-5 w-5"
                variant="ghost"
                :class="{ copied: copiedStates[model.name] }"
              >
                <Check v-if="copiedStates[model.name]" :size="12" />
                <Copy v-else :size="12" />
              </Button>
            </div>
          </div>
        </div>
        
        <div class="models-tip-glow">
          <p class="models-tip">copy any command above, paste it in your terminal, and hit enter. start with a small model - you can always add more later!</p>
        </div>
      </section>
      
      <!-- cta -->
      <section ref="ctaRef" class="cta-section">
        <Button @click="getStarted" class="cta-btn h-auto">
          <span>get started</span>
          <ChevronRight :size="22" />
        </Button>
        <p class="cta-hint">you can reopen this guide anytime from settings</p>
      </section>
      
      <!-- faq -->
      <section id="faq" ref="faqRef" class="faq-section">
        <h2 class="section-title">frequently asked questions</h2>
        
        <div class="faq-list">
          <div 
            v-for="(faq, index) in faqs" 
            :key="index"
            class="faq-item"
            :class="{ 'faq-expanded': expandedFaq === index }"
          >
            <button 
              class="faq-question"
              @click="toggleFaq(index)"
            >
              <div class="faq-icon">
                <HelpCircle :size="18" />
              </div>
              <span>{{ faq.question }}</span>
              <ChevronDown 
                :size="18" 
                class="faq-chevron"
                :class="{ 'chevron-rotated': expandedFaq === index }"
              />
            </button>
            <div class="faq-answer-wrapper" :class="{ 'answer-visible': expandedFaq === index }">
              <p class="faq-answer" v-html="formatFaqText(faq.answer)"></p>
            </div>
          </div>
        </div>
      </section>
      
      <footer class="footer">
        <p>free & open source · made with ❤️ for privacy</p>
      </footer>
    </div>
  </div>
</template>

<style>
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

.welcome-page {
  min-height: 100vh;
  background: #08080c;
  color: #e4e4e7;
  font-family: 'IBM Plex Sans', -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
  position: relative;
  overflow-x: hidden;
}

.bg-pattern {
  position: fixed;
  inset: 0;
  background: 
    radial-gradient(ellipse at 20% 0%, rgba(139, 92, 246, 0.12) 0%, transparent 50%),
    radial-gradient(ellipse at 80% 100%, rgba(6, 182, 212, 0.1) 0%, transparent 50%),
    radial-gradient(ellipse at 50% 50%, rgba(244, 63, 94, 0.05) 0%, transparent 60%);
  pointer-events: none;
}

.bg-pattern::after {
  content: '';
  position: absolute;
  inset: 0;
  background-image: radial-gradient(rgba(255,255,255,0.025) 1px, transparent 1px);
  background-size: 48px 48px;
}

.shapes-container {
  position: fixed;
  inset: 0;
  pointer-events: none;
  overflow: hidden;
  z-index: 0;
}

.float-shape {
  position: absolute;
  border-radius: 50%;
  filter: blur(60px);
  opacity: 0.4;
}

.shape-1 {
  width: 300px;
  height: 300px;
  background: linear-gradient(135deg, #8b5cf6, #06b6d4);
  top: 10%;
  left: -5%;
}

.shape-2 {
  width: 200px;
  height: 200px;
  background: linear-gradient(135deg, #f43f5e, #fbbf24);
  top: 60%;
  right: -3%;
}

.shape-3 {
  width: 150px;
  height: 150px;
  background: linear-gradient(135deg, #10b981, #06b6d4);
  bottom: 20%;
  left: 20%;
}

.content {
  position: relative;
  z-index: 1;
  max-width: 1000px;
  margin: 0 auto;
  padding: 60px 24px 40px;
}

/* hero */
.hero {
  text-align: center;
  margin-bottom: 80px;
  position: relative;
}

.hero-glow {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  width: 400px;
  height: 400px;
  background: radial-gradient(circle, rgba(139, 92, 246, 0.15) 0%, transparent 70%);
  pointer-events: none;
}

.badge {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  padding: 12px 24px;
  background: rgba(139, 92, 246, 0.12);
  border: 1px solid rgba(139, 92, 246, 0.25);
  border-radius: 100px;
  color: #a78bfa;
  font-size: 14px;
  font-weight: 600;
  margin-bottom: 36px;
  position: relative;
}

.pulse-ring {
  position: absolute;
  inset: 0;
  border-radius: 100px;
  border: 2px solid rgba(139, 92, 246, 0.4);
}

.title {
  font-size: clamp(48px, 10vw, 80px);
  font-weight: 700;
  line-height: 1.05;
  margin-bottom: 28px;
  letter-spacing: -0.03em;
}

.gradient-text {
  background: linear-gradient(135deg, #a78bfa 0%, #22d3ee 50%, #fb7185 100%);
  -webkit-background-clip: text;
  background-clip: text;
  -webkit-text-fill-color: transparent;
  display: inline-block;
}

.subtitle {
  font-size: 20px;
  color: #a1a1aa;
  max-width: 600px;
  margin: 0 auto 36px;
  line-height: 1.7;
}

.subtitle strong {
  color: #e4e4e7;
  font-weight: 500;
}

.hero-features {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 16px;
}

.hero-feature {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 14px 24px;
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 14px;
  font-size: 15px;
  font-weight: 500;
  color: #a1a1aa;
  transition: all 0.3s ease;
}

.hero-feature:hover {
  background: rgba(255, 255, 255, 0.06);
  border-color: rgba(255, 255, 255, 0.12);
  transform: translateY(-2px);
}

.hero-feature:nth-child(1) svg { color: #4ade80; }
.hero-feature:nth-child(2) svg { color: #fbbf24; }
.hero-feature:nth-child(3) svg { color: #22d3ee; }

/* section titles */
.section-title {
  font-size: 14px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.15em;
  color: #71717a;
  margin-bottom: 32px;
  text-align: center;
}

/* features */
.features-section {
  margin-bottom: 80px;
}

.features-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 20px;
}

.feature-card {
  position: relative;
  padding: 28px;
  background: rgba(255, 255, 255, 0.02);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 20px;
  overflow: hidden;
  transition: all 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275);
}

.feature-card:hover {
  transform: translateY(-6px) scale(1.02);
  border-color: rgba(255, 255, 255, 0.12);
}

.feature-bg {
  position: absolute;
  top: -50%;
  right: -50%;
  width: 200px;
  height: 200px;
  border-radius: 50%;
  opacity: 0;
  transition: opacity 0.4s;
  pointer-events: none;
}

.feature-card:hover .feature-bg {
  opacity: 0.1;
}

.feature-violet .feature-bg { background: #8b5cf6; }
.feature-cyan .feature-bg { background: #06b6d4; }
.feature-rose .feature-bg { background: #f43f5e; }
.feature-amber .feature-bg { background: #f59e0b; }
.feature-emerald .feature-bg { background: #10b981; }
.feature-blue .feature-bg { background: #3b82f6; }

.feature-icon {
  width: 52px;
  height: 52px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 14px;
  margin-bottom: 20px;
  transition: transform 0.3s;
}

.feature-card:hover .feature-icon {
  transform: scale(1.1) rotate(-3deg);
}

.icon-violet { background: rgba(139, 92, 246, 0.15); color: #a78bfa; }
.icon-cyan { background: rgba(6, 182, 212, 0.15); color: #22d3ee; }
.icon-rose { background: rgba(244, 63, 94, 0.15); color: #fb7185; }
.icon-amber { background: rgba(245, 158, 11, 0.15); color: #fbbf24; }
.icon-emerald { background: rgba(16, 185, 129, 0.15); color: #34d399; }
.icon-blue { background: rgba(59, 130, 246, 0.15); color: #60a5fa; }

.feature-content h3 {
  font-size: 18px;
  font-weight: 600;
  margin-bottom: 8px;
  color: #e4e4e7;
}

.feature-content p {
  font-size: 14px;
  color: #71717a;
  line-height: 1.6;
}

/* setup */
.setup-section {
  margin-bottom: 32px;
}

.steps {
  display: flex;
  flex-direction: column;
  gap: 24px;
}

.step {
  display: flex;
  gap: 20px;
  padding: 28px;
  background: rgba(255, 255, 255, 0.02);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 20px;
  transition: all 0.3s;
}

.step:hover {
  border-color: rgba(139, 92, 246, 0.3);
  background: rgba(255, 255, 255, 0.03);
}

.step-number {
  width: 44px;
  height: 44px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(139, 92, 246, 0.15);
  border: 1.5px solid rgba(139, 92, 246, 0.35);
  border-radius: 12px;
  font-size: 18px;
  font-weight: 700;
  color: #a78bfa;
  flex-shrink: 0;
  transition: all 0.2s ease;
}

.step:hover .step-number {
  background: rgba(139, 92, 246, 0.25);
  border-color: rgba(139, 92, 246, 0.5);
  color: #c4b5fd;
}

.step-body {
  flex: 1;
  min-width: 0;
}

.step-header {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 10px;
  flex-wrap: wrap;
}

.step-header svg {
  color: #a78bfa;
}

.step-header h3 {
  font-size: 18px;
  font-weight: 600;
  color: #e4e4e7;
}

.os-badge {
  font-size: 11px;
  font-weight: 600;
  color: #71717a;
  background: rgba(255, 255, 255, 0.05);
  padding: 4px 10px;
  border-radius: 6px;
}

.step-body > p {
  font-size: 14px;
  color: #71717a;
  margin-bottom: 16px;
}

.command-box {
  display: flex;
  gap: 12px;
  margin-bottom: 12px;
}

.command-box code {
  flex: 1;
  font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', Consolas, monospace;
  font-size: 13px;
  color: #d4d4d8;
  background: rgba(0, 0, 0, 0.4);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 12px;
  padding: 14px 18px;
  overflow-x: auto;
  white-space: nowrap;
}

.copy-btn {
  width: 50px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(139, 92, 246, 0.15);
  border: 1px solid rgba(139, 92, 246, 0.3);
  border-radius: 12px;
  color: #a78bfa;
  cursor: pointer;
  transition: all 0.2s;
  flex-shrink: 0;
}

.copy-btn:hover {
  background: rgba(139, 92, 246, 0.25);
  transform: scale(1.05);
}

.copy-btn.copied {
  background: rgba(34, 197, 94, 0.2);
  border-color: rgba(34, 197, 94, 0.4);
  color: #4ade80;
}

.link-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  background: none;
  border: none;
  color: #71717a;
  font-family: inherit;
  font-size: 13px;
  cursor: pointer;
  transition: color 0.2s;
}

.link-btn:hover {
  color: #a78bfa;
}

/* models */
.models-section {
  margin-bottom: 60px;
}

.models-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
  gap: 14px;
  margin-bottom: 20px;
}

.model-card {
  background: rgba(255, 255, 255, 0.02);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 14px;
  padding: 16px;
  transition: all 0.3s;
}

.model-card:hover {
  border-color: rgba(244, 63, 94, 0.3);
  transform: translateY(-3px);
}

.model-top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 6px;
}

.model-name {
  font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', Consolas, monospace;
  font-size: 13px;
  font-weight: 600;
  color: #e4e4e7;
}

.speed-tag {
  font-size: 9px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  padding: 3px 7px;
  border-radius: 5px;
}

.speed-tag.blazing { background: rgba(34, 197, 94, 0.15); color: #4ade80; }
.speed-tag.fast { background: rgba(251, 191, 36, 0.15); color: #fbbf24; }
.speed-tag.moderate { background: rgba(6, 182, 212, 0.15); color: #22d3ee; }

.model-desc {
  font-size: 11px;
  color: #71717a;
  margin-bottom: 12px;
}

.model-cmd {
  display: flex;
  gap: 8px;
}

.model-cmd code {
  flex: 1;
  font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', Consolas, monospace;
  font-size: 10px;
  color: #a1a1aa;
  background: rgba(0, 0, 0, 0.3);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 8px;
  padding: 8px 10px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.copy-btn-sm {
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(244, 63, 94, 0.12);
  border: 1px solid rgba(244, 63, 94, 0.25);
  border-radius: 8px;
  color: #fb7185;
  cursor: pointer;
  transition: all 0.2s;
  flex-shrink: 0;
}

.copy-btn-sm:hover {
  background: rgba(244, 63, 94, 0.2);
  transform: scale(1.08);
}

.copy-btn-sm.copied {
  background: rgba(34, 197, 94, 0.2);
  border-color: rgba(34, 197, 94, 0.4);
  color: #4ade80;
}

.models-tip-glow {
  max-width: 600px;
  margin: 0 auto;
  padding: 16px 24px;
  background: rgba(251, 191, 36, 0.05);
  border: 1px solid rgba(251, 191, 36, 0.2);
  border-radius: 12px;
  box-shadow: 0 0 30px rgba(251, 191, 36, 0.05);
  position: relative;
  overflow: hidden;
}

.models-tip-glow::before {
  content: '';
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  background: radial-gradient(circle at center, rgba(251, 191, 36, 0.1) 0%, transparent 70%);
  pointer-events: none;
}

.models-tip {
  text-align: center;
  font-size: 14px;
  color: #fbbf24;
  line-height: 1.6;
  position: relative;
  z-index: 1;
  font-weight: 500;
}

/* setup header and badges */
.setup-header {
  text-align: center;
  margin-bottom: 40px;
}

.setup-badge {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 8px 16px;
  border-radius: 100px;
  font-size: 12px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  margin-bottom: 16px;
}

.setup-badge.recommended {
  background: linear-gradient(135deg, rgba(34, 197, 94, 0.15), rgba(16, 185, 129, 0.15));
  border: 1px solid rgba(34, 197, 94, 0.3);
  color: #4ade80;
}

.setup-badge.fallback {
  background: rgba(113, 113, 122, 0.1);
  border: 1px solid rgba(113, 113, 122, 0.25);
  color: #a1a1aa;
}

.setup-desc {
  font-size: 14px;
  color: #71717a;
}

/* section divider */
.section-divider {
  display: flex;
  align-items: center;
  justify-content: center;
  margin: 60px 0;
  position: relative;
}

.section-divider::before,
.section-divider::after {
  content: '';
  flex: 1;
  height: 1px;
  background: linear-gradient(90deg, transparent, rgba(255, 255, 255, 0.08), transparent);
}

.section-divider span {
  padding: 0 24px;
  font-size: 13px;
  color: #52525b;
  font-weight: 500;
}

/* gemini status bar */
.gemini-status {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 16px 20px;
  background: rgba(255, 255, 255, 0.02);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 12px;
  margin-bottom: 32px;
  transition: all 0.3s ease;
}

.gemini-status.ready {
  background: rgba(34, 197, 94, 0.08);
  border-color: rgba(34, 197, 94, 0.25);
}

.gemini-status.downloading {
  background: rgba(59, 130, 246, 0.08);
  border-color: rgba(59, 130, 246, 0.25);
}

.gemini-status-icon {
  width: 40px;
  height: 40px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.05);
  flex-shrink: 0;
}

.gemini-status.ready .gemini-status-icon { color: #4ade80; }
.gemini-status.downloadable .gemini-status-icon { color: #60a5fa; }
.gemini-status.checking .gemini-status-icon,
.gemini-status.downloading .gemini-status-icon { color: #60a5fa; }
.gemini-status.no-browser .gemini-status-icon,
.gemini-status.unavailable .gemini-status-icon { color: #fbbf24; }

.gemini-status-content {
  flex: 1;
  min-width: 0;
}

.gemini-status-title {
  display: block;
  font-size: 14px;
  font-weight: 600;
  color: #e4e4e7;
}

.gemini-status-msg {
  display: block;
  font-size: 12px;
  color: #71717a;
  margin-top: 2px;
}

.gemini-progress {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 120px;
}

.gemini-progress-bar {
  flex: 1;
  height: 4px;
  background: rgba(255, 255, 255, 0.1);
  border-radius: 2px;
  overflow: hidden;
}

.gemini-progress-fill {
  height: 100%;
  background: linear-gradient(90deg, #3b82f6, #8b5cf6);
  border-radius: 2px;
  transition: width 0.3s ease;
}

.gemini-progress span {
  font-size: 11px;
  color: #a1a1aa;
  font-weight: 500;
}

.gemini-action-btn {
  gap: 6px !important;
  padding: 8px 16px !important;
  font-size: 13px !important;
  background: linear-gradient(135deg, #3b82f6, #8b5cf6) !important;
  border: none !important;
  color: white !important;
}

.gemini-action-btn.ready {
  background: linear-gradient(135deg, #22c55e, #10b981) !important;
}

/* flag list */
.flag-list {
  list-style: none;
  padding: 0;
  margin: 16px 0 0 0;
  font-size: 13px;
  color: #a1a1aa;
}

.flag-list li {
  padding: 8px 0;
  border-bottom: 1px solid rgba(255, 255, 255, 0.04);
}

.flag-list li:last-child {
  border-bottom: none;
}

.flag-list code {
  background: rgba(139, 92, 246, 0.12);
  color: #c4b5fd;
  padding: 2px 6px;
  border-radius: 4px;
  font-size: 11px;
}

.flag-list strong {
  color: #4ade80;
}

.recheck-btn-inline {
  margin-top: 12px;
  font-size: 13px !important;
  gap: 8px !important;
}

.troubleshoot-box {
  margin-top: 20px;
  padding: 16px;
  background: rgba(139, 92, 246, 0.08);
  border: 1px solid rgba(139, 92, 246, 0.2);
  border-radius: 10px;
}

.troubleshoot-header {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  font-weight: 600;
  color: #a78bfa;
  margin-bottom: 12px;
}

.troubleshoot-list {
  list-style: none;
  padding: 0;
  margin: 0;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.7);
}

.troubleshoot-list li {
  margin-bottom: 8px;
  line-height: 1.5;
}

.troubleshoot-list li:last-child {
  margin-bottom: 0;
}

.troubleshoot-list strong {
  color: rgba(255, 255, 255, 0.9);
}

.troubleshoot-list code {
  background: rgba(139, 92, 246, 0.2);
  padding: 2px 6px;
  border-radius: 4px;
  font-size: 11px;
  color: #c4b5fd;
}

/* spinning animation */
.spinning {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

/* ollama section styling */
.ollama-section {
  opacity: 0.9;
}

/* cta */
.cta-section {
  text-align: center;
  margin-bottom: 60px;
}

.cta-btn {
  display: inline-flex;
  align-items: center;
  gap: 14px;
  padding: 20px 48px;
  background: rgba(139, 92, 246, 0.12);
  border: 2px solid rgba(139, 92, 246, 0.4);
  border-radius: 18px;
  color: #c4b5fd;
  font-family: 'IBM Plex Sans', inherit;
  font-size: 20px;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.3s cubic-bezier(0.175, 0.885, 0.32, 1.275);
  box-shadow: 0 6px 24px rgba(139, 92, 246, 0.15);
}

.cta-btn:hover {
  transform: translateY(-4px) scale(1.02);
  background: rgba(139, 92, 246, 0.2);
  border-color: rgba(139, 92, 246, 0.6);
  color: #ddd6fe;
  box-shadow: 0 12px 40px rgba(139, 92, 246, 0.25);
}

.cta-btn svg {
  transition: transform 0.2s ease;
}

.cta-btn:hover svg {
  transform: translateX(5px);
}

.cta-hint {
  margin-top: 18px;
  font-size: 13px;
  color: #52525b;
}

/* footer */
.footer {
  padding-top: 32px;
  border-top: 1px solid rgba(255, 255, 255, 0.06);
  text-align: center;
}

.footer p {
  font-size: 13px;
  color: #52525b;
}

.step-subtitle {
  font-size: 13px;
  color: #a1a1aa;
  margin-bottom: 10px;
  font-weight: 500;
}

.step-note {
  font-size: 12px;
  color: #71717a;
  margin-top: 8px;
  margin-bottom: 16px;
  padding: 10px 14px;
  background: rgba(34, 197, 94, 0.08);
  border: 1px solid rgba(34, 197, 94, 0.15);
  border-radius: 10px;
}

.alt-method {
  margin-top: 12px;
  border-radius: 12px;
  background: rgba(255, 255, 255, 0.02);
  border: 1px solid rgba(255, 255, 255, 0.06);
}

.alt-method summary {
  padding: 12px 16px;
  font-size: 13px;
  color: #a1a1aa;
  cursor: pointer;
  transition: all 0.2s;
  list-style: none;
}

.alt-method summary::-webkit-details-marker {
  display: none;
}

.alt-method summary:hover {
  color: #e4e4e7;
  background: rgba(255, 255, 255, 0.02);
}

.alt-method[open] summary {
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
}

.manual-steps {
  padding: 16px 16px 16px 32px;
  font-size: 12px;
  color: #a1a1aa;
  line-height: 1.8;
}

.manual-steps li {
  margin-bottom: 6px;
}

.alt-desc {
  padding: 12px 16px 4px;
  font-size: 12px;
  color: #71717a;
}

.alt-method .command-box {
  margin: 12px 16px 16px;
}

/* scrollbar */
::-webkit-scrollbar { width: 6px; height: 6px; }
::-webkit-scrollbar-track { background: transparent; }
::-webkit-scrollbar-thumb { background: rgba(255, 255, 255, 0.1); border-radius: 3px; }

.faq-section {
  margin-bottom: 60px;
}

.faq-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.faq-item {
  background: rgba(255, 255, 255, 0.02);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 16px;
  overflow: hidden;
  transition: all 0.3s ease;
}

.faq-item:hover {
  border-color: rgba(139, 92, 246, 0.3);
  background: rgba(255, 255, 255, 0.03);
}

.faq-expanded {
  border-color: rgba(139, 92, 246, 0.4);
  background: rgba(139, 92, 246, 0.05);
}

.faq-question {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 18px 20px;
  background: none;
  border: none;
  color: #e4e4e7;
  font-family: 'IBM Plex Sans', inherit;
  font-size: 15px;
  font-weight: 500;
  text-align: left;
  cursor: pointer;
  transition: all 0.2s ease;
}

.faq-question:hover {
  color: #ffffff;
}

.faq-question span {
  flex: 1;
}

.faq-icon {
  width: 36px;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(139, 92, 246, 0.15);
  border-radius: 10px;
  color: #a78bfa;
  flex-shrink: 0;
  transition: all 0.3s ease;
}

.faq-expanded .faq-icon {
  background: rgba(139, 92, 246, 0.25);
  color: #c4b5fd;
}

.faq-chevron {
  color: #71717a;
  transition: transform 0.3s cubic-bezier(0.4, 0, 0.2, 1);
  flex-shrink: 0;
}

.chevron-rotated {
  transform: rotate(180deg);
  color: #a78bfa;
}

.faq-answer-wrapper {
  max-height: 0;
  overflow: hidden;
  transition: max-height 0.4s cubic-bezier(0.4, 0, 0.2, 1), 
              padding 0.3s ease,
              opacity 0.3s ease;
  opacity: 0;
}

.answer-visible {
  max-height: 300px;
  opacity: 1;
}

.faq-answer {
  padding: 0 20px 20px 70px;
  font-size: 14px;
  color: #a1a1aa;
  line-height: 1.7;
}

.inline-code {
  background: rgba(139, 92, 246, 0.15);
  color: #c4b5fd;
  padding: 2px 8px;
  border-radius: 6px;
  font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', Consolas, monospace;
  font-size: 12px;
  font-weight: 500;
  border: 1px solid rgba(139, 92, 246, 0.25);
}

.button-badge {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 4px 12px;
  background: rgba(139, 92, 246, 0.12);
  border: 1.5px solid rgba(139, 92, 246, 0.3);
  border-radius: 8px;
  color: #a78bfa;
  font-family: 'IBM Plex Sans', inherit;
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.01em;
  white-space: nowrap;
}

/* chrome ai section */
.chrome-ai-section {
  margin-bottom: 80px;
  padding-top: 60px;
  border-top: 1px solid rgba(255, 255, 255, 0.06);
}

.chrome-ai-section.primary {
  margin-top: 60px;
  background: linear-gradient(180deg, rgba(139, 92, 246, 0.03) 0%, transparent 100%);
  padding: 48px 32px;
  border-radius: 24px;
  border: 1px solid rgba(139, 92, 246, 0.12);
}

.chrome-ai-header {
  text-align: center;
  margin-bottom: 40px;
}

.chrome-ai-badge {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 10px 20px;
  background: linear-gradient(135deg, rgba(59, 130, 246, 0.12), rgba(168, 85, 247, 0.12));
  border: 1px solid rgba(139, 92, 246, 0.25);
  border-radius: 100px;
  color: #a78bfa;
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 24px;
}

.chrome-ai-badge.recommended {
  background: linear-gradient(135deg, rgba(34, 197, 94, 0.15), rgba(16, 185, 129, 0.15));
  border-color: rgba(34, 197, 94, 0.35);
  color: #4ade80;
}

/* status card */
.chrome-ai-status-card {
  display: flex;
  align-items: center;
  gap: 20px;
  padding: 24px 28px;
  background: rgba(255, 255, 255, 0.02);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 16px;
  margin-bottom: 24px;
  transition: all 0.3s ease;
}

.chrome-ai-status-card.ready {
  background: rgba(34, 197, 94, 0.08);
  border-color: rgba(34, 197, 94, 0.25);
}

.chrome-ai-status-card.downloading {
  background: rgba(59, 130, 246, 0.08);
  border-color: rgba(59, 130, 246, 0.25);
}

.status-icon-container {
  width: 48px;
  height: 48px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 12px;
  background: rgba(255, 255, 255, 0.05);
  flex-shrink: 0;
}

.status-icon.spinning {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.status-icon.success { color: #4ade80; }
.status-icon.download { color: #60a5fa; }
.status-icon.warning { color: #fbbf24; }

.status-content {
  flex: 1;
  min-width: 0;
}

.status-title {
  font-size: 16px;
  font-weight: 600;
  color: #e4e4e7;
  margin-bottom: 4px;
}

.status-message {
  font-size: 13px;
  color: #71717a;
}

.progress-container {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-top: 12px;
}

.progress-bar {
  flex: 1;
  height: 6px;
  background: rgba(255, 255, 255, 0.1);
  border-radius: 3px;
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  background: linear-gradient(90deg, #3b82f6, #8b5cf6);
  border-radius: 3px;
  transition: width 0.3s ease;
}

.progress-text {
  font-size: 12px;
  color: #a1a1aa;
  font-weight: 500;
  min-width: 40px;
  text-align: right;
}

.status-action {
  flex-shrink: 0;
}

.download-btn {
  background: linear-gradient(135deg, #3b82f6, #8b5cf6) !important;
  border: none !important;
  color: white !important;
  padding: 10px 20px !important;
  font-weight: 600 !important;
  gap: 8px !important;
}

.download-btn:hover {
  opacity: 0.9;
  transform: translateY(-1px);
}

.ready-btn {
  background: linear-gradient(135deg, #22c55e, #10b981) !important;
  border: none !important;
  color: white !important;
  padding: 10px 20px !important;
  font-weight: 600 !important;
  gap: 8px !important;
}

.setup-btn {
  border-color: rgba(255, 255, 255, 0.15) !important;
  font-size: 13px !important;
}

.recheck-btn {
  margin-top: 12px;
  font-size: 12px !important;
  gap: 6px !important;
}

.recheck-btn .spinning {
  animation: spin 1s linear infinite;
}

/* setup details accordion */
.chrome-ai-setup-details {
  margin-top: 24px;
}

.chrome-ai-setup-details summary {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 12px;
  font-size: 13px;
  color: #71717a;
  cursor: pointer;
  list-style: none;
  transition: color 0.2s;
}

.chrome-ai-setup-details summary::-webkit-details-marker {
  display: none;
}

.chrome-ai-setup-details summary:hover {
  color: #a1a1aa;
}

.chrome-ai-setup-details summary svg {
  transition: transform 0.3s;
}

.chrome-ai-setup-details[open] summary svg {
  transform: rotate(180deg);
}

.chrome-ai-setup-details .chrome-ai-grid {
  margin-top: 20px;
}

.chrome-ai-desc {
  font-size: 16px;
  color: #a1a1aa;
  max-width: 600px;
  margin: 0 auto;
  line-height: 1.7;
}

.chrome-ai-desc strong {
  color: #e4e4e7;
}

.chrome-ai-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 20px;
  margin-bottom: 32px;
}

.chrome-ai-card {
  display: flex;
  gap: 16px;
  padding: 24px;
  background: rgba(255, 255, 255, 0.02);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 16px;
  transition: all 0.3s ease;
}

.chrome-ai-card:hover {
  background: rgba(255, 255, 255, 0.04);
  border-color: rgba(255, 255, 255, 0.1);
  transform: translateY(-3px);
}

.chrome-ai-card-icon {
  width: 48px;
  height: 48px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 12px;
  flex-shrink: 0;
}

.chrome-ai-step {
  font-size: 18px;
  font-weight: 700;
}

.chrome-ai-card-content h3 {
  font-size: 15px;
  font-weight: 600;
  color: #e4e4e7;
  margin-bottom: 8px;
}

.chrome-ai-card-content p {
  font-size: 13px;
  color: #71717a;
  line-height: 1.5;
  margin-bottom: 12px;
}

.chrome-ai-card-content .command-box.small {
  padding: 8px 12px;
  margin-bottom: 8px;
}

.chrome-ai-card-content .command-box.small code {
  font-size: 11px;
}

.chrome-ai-flags {
  list-style: none;
  font-size: 12px;
  color: #a1a1aa;
  line-height: 1.8;
}

.chrome-ai-flags li {
  padding-left: 16px;
  position: relative;
}

.chrome-ai-flags li::before {
  content: "→";
  position: absolute;
  left: 0;
  color: #4ade80;
}

.chrome-ai-flags strong {
  color: #4ade80;
}

.chrome-ai-note {
  font-size: 11px !important;
  color: #52525b !important;
  font-style: italic;
}

.chrome-ai-note-box {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 16px 20px;
  background: rgba(251, 191, 36, 0.08);
  border: 1px solid rgba(251, 191, 36, 0.2);
  border-radius: 12px;
}

.chrome-ai-note-box svg {
  color: #fbbf24;
  flex-shrink: 0;
  margin-top: 2px;
}

.chrome-ai-note-box p {
  font-size: 13px;
  color: #a1a1aa;
  line-height: 1.6;
}

.chrome-ai-note-box strong {
  color: #fbbf24;
}

/* responsive */
@media (max-width: 900px) {
  .features-grid { grid-template-columns: repeat(2, 1fr); }
  .chrome-ai-grid { grid-template-columns: 1fr; }
}

@media (max-width: 640px) {
  .content { padding: 40px 16px 32px; }
  .step { flex-direction: column; gap: 16px; }
  .command-box { flex-direction: column; }
  .copy-btn { width: 100%; height: 48px; }
  .features-grid { grid-template-columns: 1fr; }
  .models-grid { grid-template-columns: repeat(2, 1fr); }
  .faq-answer { padding-left: 20px; }
  .faq-question { padding: 14px 16px; font-size: 14px; gap: 10px; }
  .faq-icon { width: 32px; height: 32px; }
  .chrome-ai-status-card { flex-direction: column; text-align: center; }
  .chrome-ai-section.primary { padding: 32px 20px; }
  .status-action { width: 100%; }
  .status-action button { width: 100%; }
}
</style>
