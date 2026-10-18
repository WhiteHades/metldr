try {
  const { default: init } = await import("./pkg/welcome_runtime.js");
  await init();
} catch {
  document.getElementById("boot-status").textContent =
    "Setup could not load. You can still read this page and open Help. Reload to try again.";
}
