<script setup>
import { ref, onMounted, onUnmounted } from 'vue';

const selectedText = ref('');

let port = null;

onMounted(() => {
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
    <main>
      <div v-if='selectedText' class='selection-box'>
        <h2>Selected Text</h2>
        <p>{{ selectedText }}</p>
      </div>
      <div v-else class='placeholder'>
        <p>select text on any page to get started</p>
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

.placeholder {
  color: #888;
  text-align: center;
  padding: 20px;
}
</style>
