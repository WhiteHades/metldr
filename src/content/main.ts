import * as InboxSDK from '@inboxsdk/core';
import { ContentScriptBootstrap } from '../content-ui/ContentScriptBootstrap';
import { INBOXSDK_APP_ID } from '../config/secrets';

const isGmail = window.location.hostname.includes('mail.google.com');

if (isGmail) {
  // initialize message listeners immediately (before InboxSDK loads)
  // this ensures GET_EMAIL_CONTENT works even during InboxSDK loading
  ContentScriptBootstrap.init(null).then(() => {
    console.log('metldr: base init done, loading inboxsdk...');
  }).catch(err => {
    console.error('metldr: base init failed:', err);
  });
  
  // then load InboxSDK and enhance with email extraction
  InboxSDK.load(2, INBOXSDK_APP_ID).then((sdk) => {
    console.log('metldr: inboxsdk loaded');
    ContentScriptBootstrap.initInboxSDK(sdk);
  }).catch(err => {
    console.error('metldr: inboxsdk load failed:', err);
  });
} else {
  ContentScriptBootstrap.init(null).catch(err => {
    console.error('metldr: init failed:', err);
  });
}
