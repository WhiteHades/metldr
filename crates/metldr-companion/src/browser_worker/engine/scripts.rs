mod buttons;
pub(super) mod selects;
pub(super) use buttons::{arm_button, button_receipt, check_button};

const HELPERS: &str = r#"
const clean = value => String(value || '').replace(/\s+/g, ' ').trim();
const bounded = (value, limit) => {
  let text = '', size = 0, truncated = false;
  for (const character of String(value || '')) {
    const bytes = new TextEncoder().encode(character).length;
    if (size + bytes > limit) { truncated = true; break; }
    text += character; size += bytes;
  }
  return [text, truncated];
};
const label = element => {
  const direct = element.getAttribute('aria-label');
  if (direct) return clean(direct);
  const labelled = element.getAttribute('aria-labelledby');
  const references = labelled ? labelled.split(/\s+/) : [];
  const labels = element.labels ? Array.from(element.labels) : [];
  if (references.length > 32 || labels.length > 32) return null;
  const referenced = references.map(id => labelText(document.getElementById(id)));
  const associated = labels.map(labelText);
  if (referenced.includes(null) || associated.includes(null)) return null;
  return clean(referenced.join(' ') || associated.join(' ') || element.getAttribute('title') || element.placeholder || '');
};
const visible = element => {
  if (!element.isConnected || element.ownerDocument !== document || element.closest('[hidden],[inert],[aria-hidden="true"]')) return false;
  let depth = 0;
  for (let node = element; node && node.nodeType === 1; node = node.parentElement) {
    if (++depth > 64) return false;
    const style = getComputedStyle(node);
    if (style.display === 'none' || style.visibility === 'hidden' || style.visibility === 'collapse' || Number(style.opacity) === 0 || style.contentVisibility === 'hidden' || style.clipPath !== 'none' || style.clip !== 'auto') return false;
  }
  return element.getClientRects().length > 0;
};
const labelText = root => {
  if (!root || !visible(root)) return '';
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
  let text = '', nodes = 0;
  while (walker.nextNode()) {
    if (++nodes > 512) return null;
    const node = walker.currentNode;
    if (node.nodeType !== Node.TEXT_NODE) continue;
    const parent = node.parentElement;
    if (!parent || parent.closest('script,style,noscript,input,textarea,select,[contenteditable]:not([contenteditable="false"])') || !visible(parent)) continue;
    text += node.nodeValue || '';
    if (text.length > 512) return null;
  }
  return text;
};
const safeLabel = element => {
  const accessible = label(element);
  if (accessible === null) return null;
  const metadataValues = [element.autocomplete, element.name, element.id, accessible, element.placeholder];
  if (metadataValues.some(value => String(value || '').length > 512)) return null;
  const metadata = metadataValues.join(' ').toLowerCase();
  if (/password|passcode|one[ -]?time[ -]?code|\botp\b|token|secret|api[ _-]?key|payment[ _-]?card|credit[ _-]?card|card[ _-]?(number|cvc|cvv)|cc[ _-]?(number|name|exp|csc|cvc|cvv)|\b(cvc|cvv)\b|social[ _-]?security|\bssn\b/.test(metadata)) return null;
  return accessible;
};
const textField = element => element instanceof HTMLTextAreaElement || element instanceof HTMLInputElement && element.type === 'text';
const eligible = element => {
  if (!textField(element) || element.matches(':disabled') || element.readOnly || !visible(element)) return null;
  return safeLabel(element);
};
const eligibleButton = element => {
  const native = element instanceof HTMLButtonElement || element instanceof HTMLInputElement && ['button','submit','reset'].includes(element.type);
  if (!native || element.matches(':disabled') || element.getAttribute('aria-disabled') === 'true' || !visible(element)) return null;
  const accessible = label(element);
  if (accessible === null) return null;
  const text = accessible || (element instanceof HTMLButtonElement ? labelText(element) : element.value);
  if (text === null) return null;
  const result = clean(text);
  return result && result.length <= 512 ? result : null;
};
"#;

fn function(body: &str) -> String {
    format!("function(root) {{ {HELPERS} {body} }}")
}

pub(super) fn capture() -> String {
    function(&format!(
        "{}\n{}",
        selects::HELPERS,
        r#"
const snapshot = [];
const buttons = [];
const selects = [];
let optionCount = 0;
Object.defineProperty(snapshot, '__metldr_buttons', { value: buttons });
Object.defineProperty(snapshot, '__metldr_selects', { value: selects });
const summary = { url: bounded(location.href, 2048)[0], title: bounded(document.title, 128)[0], text: '', text_truncated: false, fields_truncated: false, fields: [], buttons_truncated: false, buttons: [], selects_truncated: false, selects: [] };
let bytes = 0, nodes = 0;
const walker = document.createTreeWalker(document.body || document.documentElement, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
while (walker.nextNode()) {
  nodes++;
  if (nodes > 4096) { summary.text_truncated = true; summary.fields_truncated = true; summary.buttons_truncated = true; summary.selects_truncated = true; break; }
  const node = walker.currentNode;
  if (node.nodeType === Node.ELEMENT_NODE) {
    const select = captureSelect(node);
    if (select) {
      if (select.oversized || selects.length === 32 || optionCount + select.options.length > 64) summary.selects_truncated = true;
      else {
        const slot = selects.push(select) - 1;
        optionCount += select.options.length;
        summary.selects.push({ slot, label: select.label, selected: node.selectedIndex < 0 ? null : node.selectedIndex, options: select.options.map(option => ({ slot: option.index, label: option.label, enabled: option.enabled })) });
      }
    }
    const buttonLabel = eligibleButton(node);
    if (buttonLabel !== null) {
      if (buttons.length === 32) summary.buttons_truncated = true;
      else {
        const slot = buttons.push(node) - 1;
        summary.buttons.push({ slot, label: bounded(buttonLabel, 128)[0], check_label: buttonLabel });
      }
    }
    if (textField(node)) {
      const accessible = eligible(node);
      if (accessible !== null) {
        if (summary.fields.length === 32) { summary.fields_truncated = true; continue; }
        const slot = snapshot.push(node) - 1;
        const fieldLabel = bounded(accessible || `Text field ${slot + 1}`, 128)[0];
        summary.fields.push({ slot, label: fieldLabel, check_label: accessible });
      }
    }
    continue;
  }
  if (summary.text_truncated) continue;
  const parent = node.parentElement;
  if (!parent || parent.closest('script,style,noscript,input,textarea,select,[contenteditable]:not([contenteditable="false"])')) continue;
  if (!visible(parent)) continue;
  const [part, truncated] = bounded(node.nodeValue || '', 4096 - bytes);
  if (part) {
    const separator = summary.text ? ' ' : '';
    const [piece, separatorTruncated] = bounded(separator + part, 4096 - bytes);
    summary.text += piece; bytes += new TextEncoder().encode(piece).length;
    if (separatorTruncated || truncated) summary.text_truncated = true;
  }
  if (truncated && !part) summary.text_truncated = true;
}
Object.defineProperty(snapshot, '__metldr_summary', { value: summary });
return snapshot;
"#,
    ))
}

pub(super) fn read_summary() -> String {
    function("return root.__metldr_summary;")
}

pub(super) fn read_value(slot: usize, check_label: &str) -> String {
    let slot = serde_json::to_string(&slot).expect("usize serializes");
    let label = serde_json::to_string(check_label).expect("string serializes");
    function(&format!(
        "const element = root[{slot}]; const current = eligible(element); if (current === null || current !== {label} || new TextEncoder().encode(element.value).length > 1024) return null; return element.value;"
    ))
}

pub(super) fn write_value(slot: usize, check_label: &str, previous: &str, value: &str) -> String {
    let slot = serde_json::to_string(&slot).expect("usize serializes");
    let label = serde_json::to_string(check_label).expect("string serializes");
    let previous = serde_json::to_string(previous).expect("string serializes");
    let value = serde_json::to_string(value).expect("string serializes");
    function(&format!(
        "const element = root[{slot}]; const current = eligible(element); if (current === null || current !== {label} || element.value !== {previous}) return false; element.value = {value}; element.dispatchEvent(new Event('input', {{ bubbles: true }})); return true;"
    ))
}

pub(super) fn verify_value(slot: usize, check_label: &str, value: &str) -> String {
    let slot = serde_json::to_string(&slot).expect("usize serializes");
    let label = serde_json::to_string(check_label).expect("string serializes");
    let value = serde_json::to_string(value).expect("string serializes");
    function(&format!(
        "const element = root[{slot}]; const current = eligible(element); return current !== null && current === {label} && element.value === {value} ? element.value : null;"
    ))
}
