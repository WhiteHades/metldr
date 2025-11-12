import { ContentScriptBootstrap } from '../ui/ContentScriptBootstrap.js';

ContentScriptBootstrap.init().catch(err => {
  console.error('metldr: failed to initialize content script:', err);
});
