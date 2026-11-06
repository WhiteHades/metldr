import init, { mount, unmount } from "./pkg/metldr_rune_probe.js";

try {
  await init();
  mount();
  window.addEventListener("pagehide", unmount, { once: true });
  document.body.dataset.boot = "requested";
} catch (error) {
  document.body.dataset.boot = "failed";
  document.body.textContent = String(error);
}
