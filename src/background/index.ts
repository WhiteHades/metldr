import '@inboxsdk/core/background';

import { BackgroundBootstrap } from '../services/BackgroundBootstrap';

BackgroundBootstrap.init().catch(err => {
  console.error('metldr: failed to initialize background service:', err);
});
