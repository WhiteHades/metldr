pub(super) const HELPERS: &str = r#"
const selectLabel = element => {
  if (!(element instanceof HTMLSelectElement) || element.multiple || element.matches(':disabled') || element.getAttribute('aria-disabled') === 'true' || !visible(element)) return null;
  return safeLabel(element);
};
const optionEnabled = option => {
  if (option.disabled || option.closest('optgroup[disabled],[hidden],[inert],[aria-hidden="true"]')) return false;
  for (let node = option; node instanceof HTMLOptionElement || node instanceof HTMLOptGroupElement; node = node.parentElement) {
    const style = getComputedStyle(node);
    if (style.display === 'none' || style.visibility === 'hidden' || style.visibility === 'collapse') return false;
  }
  return true;
};
const selectMetadata = element => JSON.stringify([element.id, element.name, element.autocomplete, element.getAttribute('form')]);
const captureSelect = element => {
  const accessible = selectLabel(element);
  if (accessible === null || !element.options.length) return null;
  if (element.options.length > 32) return { oversized: true };
  const metadata = selectMetadata(element);
  if (metadata.length > 4096) return { oversized: true };
  const options = [];
  for (const node of element.options) {
    const check_label = clean(node.label);
    const value = node.value;
    const parent = node.parentElement;
    const group_label = parent instanceof HTMLOptGroupElement ? parent.label : null;
    if (check_label.length > 512 || node.id.length > 512 || group_label?.length > 512 || new TextEncoder().encode(value).length > 1024) return { oversized: true };
    const index = options.length;
    options.push({ node, index, id: node.id, parent, group_label, check_label, label: bounded(check_label || `Option ${index + 1}`, 128)[0], value, enabled: optionEnabled(node) });
  }
  if (!options.some(option => option.enabled)) return null;
  return { element, metadata, form: element.form, check_label: accessible, label: bounded(accessible || 'Selection', 128)[0], options };
};
const checkedSelect = (root, slot) => {
  const saved = root.__metldr_selects?.[slot];
  if (!saved || selectLabel(saved.element) !== saved.check_label || selectMetadata(saved.element) !== saved.metadata || saved.element.form !== saved.form || saved.element.options.length !== saved.options.length) return null;
  for (let index = 0; index < saved.options.length; index++) {
    const option = saved.options[index];
    const parent = option.node.parentElement;
    const group_label = parent instanceof HTMLOptGroupElement ? parent.label : null;
    if (saved.element.options[index] !== option.node || option.node.closest('select') !== saved.element || option.node.id !== option.id || parent !== option.parent || group_label !== option.group_label || clean(option.node.label) !== option.check_label || option.node.value !== option.value || optionEnabled(option.node) !== option.enabled) return null;
  }
  return saved;
};
"#;

fn function(body: &str) -> String {
    super::function(&format!("{HELPERS}\n{body}"))
}

pub(in crate::browser_worker::engine) fn read(slot: usize, option: usize) -> String {
    function(&format!(
        "const saved = checkedSelect(root, {slot}); if (!saved || !saved.options[{option}]?.enabled) return null; return {{selected: saved.element.selectedIndex < 0 ? null : saved.element.selectedIndex}};"
    ))
}

pub(in crate::browser_worker::engine) fn write(
    slot: usize,
    previous: Option<usize>,
    option: usize,
) -> String {
    let previous = serde_json::to_string(&previous).expect("selection index serializes");
    function(&format!(
        "const saved = checkedSelect(root, {slot}); if (!saved || !saved.options[{option}]?.enabled || (saved.element.selectedIndex < 0 ? null : saved.element.selectedIndex) !== {previous}) return false; if (saved.element.selectedIndex === {option}) return true; saved.element.selectedIndex = {option}; saved.element.dispatchEvent(new Event('input', {{bubbles:true}})); saved.element.dispatchEvent(new Event('change', {{bubbles:true}})); return true;"
    ))
}

pub(in crate::browser_worker::engine) fn verify(slot: usize, option: usize) -> String {
    function(&format!(
        "const saved = checkedSelect(root, {slot}); return !!saved && !!saved.options[{option}]?.enabled && saved.element.selectedIndex === {option} && saved.options[{option}].node.selected;"
    ))
}
