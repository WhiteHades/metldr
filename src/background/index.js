import '@inboxsdk/core/background';

import { BackgroundBootstrap } from '../services/BackgroundBootstrap.js';

BackgroundBootstrap.init().catch(err => {
  console.error('metldr: failed to initialize background service:', err);
});
