<script setup>
import { ref, onMounted, onUnmounted } from 'vue';

const ollamaStatus = ref('checking');
const setupCommands = `curl -fsSL https://ollama.com/install.sh | sh
OLLAMA_ORIGINS="chrome-extension://*" ollama serve`;

let statusCheckInterval = null;

const OLLAMA_URL = 'http://127.0.0.1:11434';

async function checkOllama() {
  try {
    const response = await fetch(`${OLLAMA_URL}/api/tags`, {
      method: 'GET',
      signal: AbortSignal.timeout(2000)
    });
    
    ollamaStatus.value = response.ok ? 'ready' : 'error';
    return response.ok;
  } catch (error) {
    ollamaStatus.value = 'not-found';
    return false;
  }
}

async function retryDetection() {
  ollamaStatus.value = 'checking';
  await checkOllama();
}

onMounted(async () => {
  await checkOllama();

  statusCheckInterval = setInterval(async () => {
    if (ollamaStatus.value !== 'ready') {
      await checkOllama();
    }
  }, 5000);
});

onUnmounted(() => {
  if (statusCheckInterval) clearInterval(statusCheckInterval);
});
</script>

<template>
  <div class="container">
    <header>
      <h1>MeTLDR</h1>
      <p class="subtitle">local and private ai assistant</p>
    </header>

    <main>
      <div v-if="ollamaStatus === 'checking'" class="status-box">
        <p>checking for ollama...</p>
      </div>

      <div v-else-if="ollamaStatus === 'not-found'" class="not-found">
        <p>ollama not detected</p>
        <button @click="retryDetection" class="secondary-btn">
          check again
        </button>
      </div>

      <div v-else-if="ollamaStatus === 'ready'" class="dashboard">
        <div class="status-indicator">
          <span class="status-dot"></span>
          <span>ollama ready</span>
        </div>

        <div class="welcome-card">
          <h2>Welcome to MeTLDR</h2>
          <p>local, private AI assistant.</p>
        </div>
      </div>

      <div v-else-if="ollamaStatus === 'error'" class="error-box">
        <p>connection error</p>
        <button @click="retryDetection" class="secondary-btn">
          check again
        </button>
      </div>
    </main>
  </div>
</template>

<style scoped>
.container {
  padding: 16px;
  width: 100%;
  box-sizing: border-box;
}

header {
  border-bottom: 1px solid #444;
  padding-bottom: 12px;
  margin-bottom: 20px;
}

h1 {
  font-size: 1.8em;
  margin: 0;
  color: #fff;
}

.subtitle {
  font-size: 0.9em;
  color: #888;
  margin: 4px 0 0;
}

h2 {
  font-size: 1.2em;
  margin: 0 0 12px 0;
}

h3 {
  font-size: 1em;
  margin: 12px 0 8px 0;
}

.status-box {
  text-align: center;
  padding: 40px 20px;
  color: #ccc;
}

.not-found {
  text-align: center;
  padding: 40px 20px;
  color: #ccc;
}

.status-indicator {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px;
  background-color: #2a3a2a;
  border: 1px solid #5a5;
  border-radius: 8px;
  margin-bottom: 20px;
}

.status-dot {
  width: 8px;
  height: 8px;
  background-color: #5a5;
  border-radius: 50%;
  animation: pulse 2s ease-in-out infinite;
}

.error-box {
  text-align: center;
  padding: 40px 20px;
  color: #a55;
}
</style>
