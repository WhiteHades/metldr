<script setup>
import { ref, onMounted, onUnmounted } from 'vue';

const selectedText = ref('');
const isGpuAvailable = ref(false);
let port = null;

onMounted(() => {
  if (navigator.gpu) {
    console.log("gpu available")
    isGpuAvailable.value = true;
  }
  else {
    console.log("gpu not available")
  }

  port = chrome.runtime.connect({ name: "side_panel" });

  port.onMessage.addListener((message) => {
    if (message.type === 'SELECTION_TEXT') {
      selectedText.value = message.text;
    }
  });
})

onUnmounted(() => {
  if (port) {
    port.disconnect();
  }
})

</script>

<template>
  <div class="container">
    <header>
      <h1>
        MeTLDR
        <p class="subtitle">local and private ai assistant</p>
      </h1>
    </header>
    <main v-if='isGpuAvailable'>
      <div v-if='selectedText' class='selection-box'>
        <h2>Selected Text</h2>
        <p>{{ selectedText }}</p>
      </div>
      <div v-else class='placeholder'>
        <p>select text on any page to get started</p>
      </div>
    </main>
    <main v-else class='errorbox'>
      <h2>WebGPU not available</h2>
      <p>MeTLDR requires the WebGPU to run AI models locally.</p>
      <p>Please update to the latest version of Chrome or enable WebGPU in <code>chrome://flags</code></p>
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
  padding-bottom: 8px;
  margin-bottom: 16px;
}

h1 {
  font-size: 1.5em;
  margin: 0;
}

.subtitle {
  font-size: 0.9em;
  color: #888;
  margin: 4px 0 0;
}

.selection-box {
  background-color: #333;
  border-radius: 8px;
  padding: 12px;
  font-family: monospace;
  white-space: pre-wrap;
  word-wrap: break-word;
}

.errorbox {
  background-color: #4d2d2d;
  border: 1px solid #a55;
  border-radius: 8px;
  padding: 16px;
  text-align: center;
}

.placeholder {
  color: #888;
  text-align: center;
  padding: 20px;
}
</style>
