import * as InboxSDK from '@inboxsdk/core';
import { ContentScriptBootstrap } from '../ui/ContentScriptBootstrap.js';
import { INBOXSDK_APP_ID } from '../config/secrets.js';

const isGmail = window.location.hostname.includes('mail.google.com');

if (isGmail) {
  InboxSDK.load(2, INBOXSDK_APP_ID).then((sdk) => {
    console.log('metldr: inboxsdk loaded');
    ContentScriptBootstrap.init(sdk).catch(err => {
      console.error('metldr: init failed:', err);
    });
  }).catch(err => {
    console.error('metldr: inboxsdk load failed:', err);
    ContentScriptBootstrap.init(null).catch(e => {
      console.error('metldr: fallback init failed:', e);
    });
  });
} else {
  ContentScriptBootstrap.init(null).catch(err => {
    console.error('metldr: init failed:', err);
  });
}
