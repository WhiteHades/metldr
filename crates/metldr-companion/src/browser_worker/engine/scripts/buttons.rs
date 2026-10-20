use super::function;

fn button(slot: usize, label: &str) -> String {
    let label = serde_json::to_string(label).expect("string serializes");
    format!(
        "const element = root.__metldr_buttons[{slot}]; const current = eligibleButton(element); if (current === null || current !== {label}) return null;"
    )
}

pub(in super::super) fn check_button(slot: usize, label: &str) -> String {
    function(&format!("{} return current;", button(slot, label)))
}

pub(in super::super) fn arm_button(slot: usize, label: &str) -> String {
    function(&format!(
        r#"
if (root.__metldr_button_receipt) root.__metldr_button_receipt.dispose();
{}
const rect = element.getBoundingClientRect();
const x = rect.x + rect.width / 2, y = rect.y + rect.height / 2;
if (!Number.isFinite(x) || !Number.isFinite(y) || x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) return null;
const hit = document.elementFromPoint(x, y);
if (!hit || !(hit === element || element.contains(hit))) return null;
const baseline = performance.eventCounts?.get('click');
if (!Number.isSafeInteger(baseline)) return null;
let count = 0, matches = true, disposed = false;
const listener = event => {{
  if (!event.isTrusted) return;
  count++;
  matches = matches && event.composedPath().includes(element);
}};
const dispose = () => {{
  if (disposed) return;
  disposed = true;
  window.removeEventListener('click', listener, true);
  clearTimeout(timer);
}};
const timer = setTimeout(dispose, 8000);
window.addEventListener('click', listener, true);
root.__metldr_button_receipt = {{ dispose, read: () => {{ const valid = !disposed && count === 1 && matches && performance.eventCounts.get('click') === baseline + 1; dispose(); return valid; }} }};
return {{x,y}};
"#,
        button(slot, label)
    ))
}

pub(in super::super) fn button_receipt() -> String {
    function(
        "const receipt = root.__metldr_button_receipt; if (!receipt) return false; delete root.__metldr_button_receipt; return receipt.read();",
    )
}
