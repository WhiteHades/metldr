export function capturePage() {
  const omitted = (element) => {
    if (element.matches("script,style,noscript,template,h1,h2,h3,form,input,textarea,select,button,nav,[hidden],[inert],[aria-hidden='true'],[role='textbox']") || element.isContentEditable) return true;
    const style = getComputedStyle(element);
    return style.display === "none" || style.visibility !== "visible" || Number(style.opacity) === 0 || style.contentVisibility === "hidden" || style.clipPath !== "none" || style.clip !== "auto";
  };
  const included = (candidate) => {
    let depth = 0;
    for (let element = candidate; element; element = element.parentElement) {
      if (++depth > 64 || omitted(element)) return false;
    }
    return true;
  };
  const articles = document.getElementsByTagName("article");
  let root = document.body;
  for (let index = 0; index < Math.min(articles.length, 32); index++) {
    if (included(articles[index])) { root = articles[index]; break; }
  }
  if (!root || !included(root)) throw new Error("Page content is unavailable.");
  let nodes = 0;
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      if (++nodes > 4096) throw new Error("Page content exceeds the capture limit.");
      if (node.nodeType === Node.ELEMENT_NODE) return omitted(node) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_SKIP;
      return NodeFilter.FILTER_ACCEPT;
    },
  });
  const parts = [];
  let size = 0;
  let previousBlock;
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const text = node.data.replace(/\s+/gu, " ");
    if (!text) continue;
    const range = document.createRange();
    range.selectNodeContents(node);
    if (![...range.getClientRects()].some((rect) => rect.width > 0 && rect.height > 0)) continue;
    let block = node.parentElement;
    while (block && block !== root && ["inline", "inline-block", "contents"].includes(getComputedStyle(block).display)) block = block.parentElement;
    const separator = previousBlock && previousBlock !== block ? " " : "";
    size += text.length + separator.length;
    if (size > 8192) throw new Error("Page content exceeds the capture limit.");
    parts.push(separator, text);
    previousBlock = block;
  }
  return {
    url: location.href,
    title: document.title,
    text: parts.join("").trim().replace(/\s+/gu, " "),
  };
}
