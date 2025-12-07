<script setup>
import { ref, computed, onMounted } from 'vue';
import { useThemeStore } from '../stores/theme.js';
import { getOsType, getSetupCommands } from '../utils/platformUtils.js';
import { gsap } from 'gsap';
import { 
  Download, Terminal, Cpu, Sparkles, Check, Copy, 
  ChevronRight, ExternalLink, Zap, Shield, Globe,
  FileText, MessageCircle, Mail, Reply, BookOpen, Languages
} from 'lucide-vue-next';

const themeStore = useThemeStore();
const detectedOs = ref(getOsType());
const platformSetup = computed(() => getSetupCommands());
const copiedStates = ref({});

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
  { name: 'gemma3:1b', size: '1b', desc: 'ultra-fast, lightweight', speed: 'fast_' },
  { name: 'qwen3:1.4b', size: '1.4b', desc: 'compact & capable', speed: 'fast_' },
  { name: 'gemma3n:e2b', size: '2b', desc: 'efficient edge model', speed: 'fast' },
  { name: 'llama3.2:1b', size: '1b', desc: 'meta\'s compact model', speed: 'fast_' },
  { name: 'deepseek-r1:1.5b', size: '1.5b', desc: 'reasoning focused', speed: 'fast' },
  { name: 'qwen3:1.7b', size: '1.7b', desc: 'balanced performance', speed: 'fast' },
  { name: 'ministral-3b', size: '3b', desc: 'mistral\'s mini model', speed: 'fast' },
  { name: 'llama3.2:3b', size: '3b', desc: 'great for most tasks', speed: 'moderate' },
  { name: 'gemma3:4b', size: '4b', desc: 'google\'s quality model', speed: 'moderate' },
  { name: 'gemma3n:e4b', size: '4b', desc: 'edge optimized', speed: 'moderate' },
];

const heroRef = ref(null);
const featuresRef = ref(null);
const stepsRef = ref(null);
const modelsRef = ref(null);
const ctaRef = ref(null);

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
          <Sparkles :size="16" />
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
      
      <!-- setup steps -->
      <section class="setup-section">
        <h2 class="section-title">quick setup</h2>
        
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
                <button 
                  @click="copyToClipboard(platformSetup.install, 'install')"
                  class="copy-btn"
                  :class="{ copied: copiedStates['install'] }"
                >
                  <Check v-if="copiedStates['install']" :size="16" />
                  <Copy v-else :size="16" />
                </button>
              </div>
              
              <button @click="openOllamaWebsite" class="link-btn">
                or download from ollama.com
                <ExternalLink :size="12" />
              </button>
            </div>
          </div>
          
          <!-- step 2 -->
          <div class="step">
            <div class="step-number">2</div>
            <div class="step-body">
              <div class="step-header">
                <Terminal :size="20" />
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
                  <button 
                    @click="copyToClipboard(platformSetup.permanentSetup.command, 'permanent')"
                    class="copy-btn"
                    :class="{ copied: copiedStates['permanent'] }"
                  >
                    <Check v-if="copiedStates['permanent']" :size="16" />
                    <Copy v-else :size="16" />
                  </button>
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
                  <button 
                    @click="copyToClipboard(platformSetup.permanentSetup.command, 'permanent')"
                    class="copy-btn"
                    :class="{ copied: copiedStates['permanent'] }"
                  >
                    <Check v-if="copiedStates['permanent']" :size="16" />
                    <Copy v-else :size="16" />
                  </button>
                </div>
                <p class="step-note">then quit ollama from the menu bar (click the llama icon → quit) and relaunch it.</p>
              </template>
              
              <!-- linux -->
              <template v-else>
                <p class="step-subtitle">{{ platformSetup.permanentSetup.commandNote }}</p>
                <div v-for="(cmd, i) in platformSetup.permanentSetup.commands" :key="i" class="command-box" style="margin-bottom: 8px;">
                  <code>{{ cmd }}</code>
                  <button 
                    @click="copyToClipboard(cmd, `linux-${i}`)"
                    class="copy-btn"
                    :class="{ copied: copiedStates[`linux-${i}`] }"
                  >
                    <Check v-if="copiedStates[`linux-${i}`]" :size="16" />
                    <Copy v-else :size="16" />
                  </button>
                </div>
              </template>
              
              <!-- quick test fallback -->
              <details class="alt-method">
                <summary>
                  <span>⚡ quick test (temporary, this session only)</span>
                </summary>
                <p class="alt-desc">if you just want to test quickly, run this in a terminal. note: you'll need to run it again each time.</p>
                <div class="command-box">
                  <code>{{ platformSetup.serve }}</code>
                  <button 
                    @click="copyToClipboard(platformSetup.serve, 'serve')"
                    class="copy-btn"
                    :class="{ copied: copiedStates['serve'] }"
                  >
                    <Check v-if="copiedStates['serve']" :size="16" />
                    <Copy v-else :size="16" />
                  </button>
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
              <button 
                @click="copyToClipboard(`ollama pull ${model.name}`, model.name)"
                class="copy-btn-sm"
                :class="{ copied: copiedStates[model.name] }"
              >
                <Check v-if="copiedStates[model.name]" :size="12" />
                <Copy v-else :size="12" />
              </button>
            </div>
          </div>
        </div>
        
        <p class="models-tip">💡 copy any command above, paste it in your terminal, and hit enter. start with a small model - you can always add more later!</p>
      </section>
      
      <!-- cta -->
      <section ref="ctaRef" class="cta-section">
        <button @click="getStarted" class="cta-btn">
          <span>get started</span>
          <ChevronRight :size="22" />
        </button>
        <p class="cta-hint">you can reopen this guide anytime from settings</p>
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
  font-family: 'Space Grotesk', -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
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
  background: linear-gradient(135deg, #8b5cf6, #06b6d4);
  border-radius: 12px;
  font-size: 18px;
  font-weight: 700;
  color: #fff;
  flex-shrink: 0;
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

.models-tip {
  text-align: center;
  font-size: 14px;
  color: #71717a;
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
  background: linear-gradient(135deg, #8b5cf6 0%, #06b6d4 100%);
  border: none;
  border-radius: 18px;
  color: #fff;
  font-family: 'Space Grotesk', inherit;
  font-size: 20px;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.3s cubic-bezier(0.175, 0.885, 0.32, 1.275);
  box-shadow: 0 10px 40px rgba(139, 92, 246, 0.35);
}

.cta-btn:hover {
  transform: translateY(-4px) scale(1.03);
  box-shadow: 0 16px 50px rgba(139, 92, 246, 0.45);
}

.cta-btn svg {
  transition: transform 0.2s;
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

/* responsive */
@media (max-width: 900px) {
  .features-grid { grid-template-columns: repeat(2, 1fr); }
}

@media (max-width: 640px) {
  .content { padding: 40px 16px 32px; }
  .step { flex-direction: column; gap: 16px; }
  .command-box { flex-direction: column; }
  .copy-btn { width: 100%; height: 48px; }
  .features-grid { grid-template-columns: 1fr; }
  .models-grid { grid-template-columns: repeat(2, 1fr); }
}
</style>
