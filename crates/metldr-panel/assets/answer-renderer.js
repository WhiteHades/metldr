(() => {
  const createAnswerRenderer = ({ insert, onRendered }) => {
    const parseHtml = new DOMParser().parseFromString.bind(new DOMParser());
    const generatedStyles = document.createElement("style");
    document.head.append(generatedStyles);
    const extraStyles = document.createElement("style");
    document.head.append(extraStyles);

    function updateDocument(html, css, complete) {
      const parsed = parseHtml(html, "text/html");
      if (parsed.querySelectorAll("*").length > 8192) throw new Error("Answer contains too many elements.");
      const scripts = Array.from(parsed.querySelectorAll("script"));
      scripts.forEach(script => script.remove());
      parsed.querySelectorAll("iframe, frame, object, embed, base, meta, link").forEach(node => node.remove());
      const styles = Array.from(parsed.querySelectorAll("style"));
      extraStyles.textContent = styles.map(style => style.textContent ?? "").join("\n");
      styles.forEach(style => style.remove());
      parsed.querySelectorAll("*").forEach(element => {
        for (const attribute of Array.from(element.attributes)) {
          const name = attribute.name.toLowerCase();
          if (name.startsWith("on") || name === "href" || name === "xlink:href") element.removeAttribute(attribute.name);
        }
      });
      generatedStyles.textContent = css;
      document.body.inert = !complete;
      insert(parsed.body.innerHTML);
      onRendered(complete);
      if (complete) {
        for (const script of scripts) {
          if (script.src) throw new Error("Answer scripts must be self contained.");
          if (script.type && script.type !== "text/javascript" && script.type !== "application/javascript") {
            throw new Error("Answer script type is not supported.");
          }
        }
      }
      return complete ? scripts.map(script => script.textContent ?? "") : [];
    }

    return { updateDocument };
  };
  Object.defineProperty(globalThis, "metldrCreateAnswerRenderer", {
    value: createAnswerRenderer, writable: false, configurable: false,
  });
})();
