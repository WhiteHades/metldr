import init, { accept_panel } from "../crates/metldr-panel/pkg/metldr_panel.js";

const waiting = new Set();
let status = "loading";

chrome.runtime.onConnect.addListener((port) => {
  if (port.name !== "metldr-first-flow") return;
  if (status === "ready") accept_panel(port);
  else if (status === "failed" || waiting.size >= 16) port.disconnect();
  else {
    waiting.add(port);
    port.onDisconnect.addListener(() => waiting.delete(port));
  }
});

chrome.commands.onCommand.addListener((command, tab) => {
  const path = command === "open-browser-agent" ? "panel.html"
    : command === "open-page-tools" ? "index.html" : undefined;
  if (!path || !Number.isInteger(tab?.id) || tab.id < 0 || tab.id > 2147483647) return;
  chrome.sidePanel.setOptions({ tabId: tab.id, path, enabled: true }, () => {
    if (chrome.runtime.lastError) {
      console.error("MetLDR could not select its panel.", chrome.runtime.lastError.message);
      return;
    }
    chrome.sidePanel.open({ tabId: tab.id }).catch((error) => {
      console.error("MetLDR could not open its panel.", error);
    });
  });
});

init().then(() => {
  status = "ready";
  for (const port of waiting) accept_panel(port);
  waiting.clear();
}).catch((error) => {
  status = "failed";
  for (const port of waiting) port.disconnect();
  waiting.clear();
  console.error("MetLDR could not load its Rust panel module.", error);
});
