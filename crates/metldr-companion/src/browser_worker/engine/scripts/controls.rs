use super::function;
use crate::browser_action::Control;

pub(super) const HELPERS: &str = r#"
const nativeMembers = element => {
  if (!element.name) return [element];
  if (element.name.length > 512) return null;
  const inputs = document.querySelectorAll('input[type="radio" i]');
  if (inputs.length > 128) return null;
  const members = Array.from(inputs).filter(peer => peer.type === 'radio' && peer.name === element.name && peer.form === element.form && peer.getRootNode() === element.getRootNode());
  return members.length > 0 && members.length <= 32 ? members : null;
};
const captureControl = (element, slot) => {
  if (!(element instanceof HTMLInputElement) || !['checkbox','radio'].includes(element.type) || element.matches(':disabled') || element.getAttribute('aria-disabled') === 'true' || !visible(element)) return null;
  const checkLabel = safeLabel(element);
  if (checkLabel === null || element.name.length > 512) return {blocked:true};
  const kind = element.type;
  const displayLabel = bounded(checkLabel || `${kind === 'checkbox' ? 'Checkbox' : 'Radio'} ${slot + 1}`, 128)[0];
  const saved = {element, slot, kind, label:displayLabel, checkLabel, name:element.name, form:element.form, tree:element.getRootNode(), id:element.id};
  if (kind === 'checkbox') {
    saved.state = {kind, checked:element.checked, indeterminate:element.indeterminate};
  } else {
    const members = nativeMembers(element);
    if (!members) return {blocked:true};
    saved.members = members.map((peer,index) => {
      const rawLabel = safeLabel(peer);
      return {element:peer, id:peer.id, index, checkLabel:rawLabel, label:bounded(rawLabel || `Radio option ${index + 1}`,128)[0], hidden:!visible(peer), disabled:peer.matches(':disabled'), checked:peer.checked};
    });
    const index = members.indexOf(element), selected = saved.members.filter(peer => peer.checked);
    if (index < 0 || selected.length > 1) return {blocked:true};
    saved.label = saved.members[index].label;
    const peer = selected[0];
    saved.state = {kind, checked:element.checked, index, group:{member_count:members.length, prior_selected:peer ? {index:peer.index,label:peer.label,hidden:peer.hidden,disabled:peer.disabled} : null}};
  }
  return saved;
};
const currentControl = (saved, after) => {
  const current = captureControl(saved.element, saved.slot);
  if (!current || current.blocked || current.kind !== saved.kind || current.label !== saved.label || current.checkLabel !== saved.checkLabel || current.name !== saved.name || current.form !== saved.form || current.tree !== saved.tree || current.id !== saved.id) return null;
  if (saved.kind === 'radio') {
    if (current.members.length !== saved.members.length || current.state.index !== saved.state.index) return null;
    if (saved.members.some((peer,index) => {
      const next = current.members[index];
      return next.element !== peer.element || next.id !== peer.id || next.checkLabel !== peer.checkLabel || next.label !== peer.label || next.hidden !== peer.hidden || next.disabled !== peer.disabled || (!after && next.checked !== peer.checked);
    })) return null;
  }
  if (!after && JSON.stringify(current.state) !== JSON.stringify(saved.state)) return null;
  return current;
};
"#;

fn script(body: &str) -> String {
    function(&format!("{HELPERS}\n{body}"))
}

pub(in super::super) fn read(slot: usize) -> String {
    script(&format!(
        "const saved = root.__metldr_controls[{slot}]; if (!saved) return null; const current = currentControl(saved,false); return current ? current.state : null;"
    ))
}

pub(in super::super) fn arm(slot: usize, control: &Control) -> String {
    let control = serde_json::to_string(control).expect("control serializes");
    script(&format!(
        r#"
const saved = root.__metldr_controls[{slot}];
const proposed = {control};
if (!saved || !currentControl(saved,false) || proposed.control_label !== saved.label || proposed.kind !== saved.kind) return null;
if (proposed.kind === 'checkbox') {{
  if (JSON.stringify(proposed.before) !== JSON.stringify({{checked:saved.state.checked,indeterminate:saved.state.indeterminate}}) || proposed.checked === saved.state.checked) return null;
}} else if (saved.state.checked || proposed.index !== saved.state.index || JSON.stringify(proposed.group) !== JSON.stringify(saved.state.group)) return null;
if (root.__metldr_control_receipt) root.__metldr_control_receipt.dispose();
const element = saved.element, rect = element.getBoundingClientRect();
const x = rect.x + rect.width / 2, y = rect.y + rect.height / 2;
if (!Number.isFinite(x) || !Number.isFinite(y) || x < 0 || y < 0 || x >= innerWidth || y >= innerHeight || document.elementFromPoint(x,y) !== element) return null;
const baseline = performance.eventCounts?.get('click');
if (!Number.isSafeInteger(baseline)) return null;
const counts = {{click:0,input:0,change:0}};
let matches = true, disposed = false;
const listener = event => {{
  if (!event.isTrusted) return;
  counts[event.type]++;
  matches = matches && event.composedPath().includes(element);
}};
const dispose = () => {{
  if (disposed) return;
  disposed = true;
  for (const type of ['click','input','change']) window.removeEventListener(type,listener,true);
  clearTimeout(timer);
}};
const timer = setTimeout(dispose,8000);
for (const type of ['click','input','change']) window.addEventListener(type,listener,true);
root.__metldr_control_receipt = {{dispose, read:() => {{
  const events = !disposed && matches && counts.click === 1 && counts.input === 1 && counts.change === 1 && performance.eventCounts.get('click') === baseline + 1;
  dispose();
  const current = currentControl(saved,true);
  if (!events || !current) return null;
  if (saved.kind === 'checkbox') {{
    if (current.state.checked !== proposed.checked || current.state.indeterminate) return null;
    return {{kind:'checkbox',checked:current.state.checked,indeterminate:current.state.indeterminate}};
  }}
  if (!current.state.checked || current.members.some(peer => peer.checked !== (peer.index === proposed.index))) return null;
  return {{kind:'radio',selected:current.state.group.prior_selected,member_count:current.members.length,cleared_peer:proposed.group.prior_selected}};
}} }};
return {{x,y}};
"#
    ))
}

pub(in super::super) fn receipt() -> String {
    script(
        "const receipt = root.__metldr_control_receipt; if (!receipt) return null; delete root.__metldr_control_receipt; return receipt.read();",
    )
}
