import { createApp } from 'vue';
import { createPinia } from 'pinia';
import Welcome from './Welcome.vue';

const app = createApp(Welcome);
app.use(createPinia());
app.mount('#welcome-app');
