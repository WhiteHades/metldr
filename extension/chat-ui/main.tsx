import { mountChat } from "./conversation";
import { capturePage } from "../page-capture.js";

const container = document.getElementById("chat-root");
if (!container) throw new Error("The chat container is missing.");
try { await mountChat(container, {capture: capturePage}); }
catch (error) {
  container.textContent = "metldr could not connect. " + String(error);
  const reload = document.createElement("button");
  reload.type = "button"; reload.textContent = "Reload";
  reload.addEventListener("click", () => location.reload());
  container.append(reload);
}
