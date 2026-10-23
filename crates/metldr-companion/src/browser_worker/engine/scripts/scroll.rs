use crate::browser_action::Direction;

fn function(body: &str) -> String {
    super::function(
        &(format!(
            r#"
const currentScroller = (root, slot, expected, checkPosition = true) => {{
  const saved = root.__metldr_scrollers?.[slot];
  if (!saved || saved.document !== document || saved.url !== expected.url || location.href !== expected.url || saved.kind !== expected.kind || saved.label !== expected.label) return null;
  if (saved.x !== expected.x || saved.y !== expected.y) return null;
  const element = saved.element;
  if (!element || !element.isConnected || element.ownerDocument !== document || element.parentElement !== saved.parent || (saved.kind === 'region' && !visible(element))) return null;
  if ((saved.kind === 'page' ? document.scrollingElement : element) !== element) return null;
  const style = getComputedStyle(element);
  if (style.overflowX !== saved.overflowX || style.overflowY !== saved.overflowY || element.scrollWidth !== saved.width || element.scrollHeight !== saved.height || element.clientWidth !== saved.clientWidth || element.clientHeight !== saved.clientHeight) return null;
  if (checkPosition && (element.scrollLeft !== expected.x || element.scrollTop !== expected.y)) return null;
  const accessible = label(element);
  const labelNow = bounded(accessible || (saved.kind === 'page' ? 'Page' : `Scrollable region ${{slot}}`), 128)[0];
  if (labelNow !== saved.label) return null;
  return {{ saved, element }};
}};
"#
        ) + body),
    )
}

pub(in crate::browser_worker::engine) fn read(
    slot: usize,
    kind: &str,
    label: &str,
    url: &str,
    x: &serde_json::Number,
    y: &serde_json::Number,
) -> String {
    let expected = serde_json::json!({"kind":kind,"label":label,"url":url,"x":x,"y":y});
    let expected = serde_json::to_string(&expected).expect("scroll observation serializes");
    function(&format!(
        "const current = currentScroller(root, {slot}, {expected}); return current ? {{x: current.element.scrollLeft, y: current.element.scrollTop}} : null;"
    ))
}

pub(in crate::browser_worker::engine) fn write(
    slot: usize,
    kind: &str,
    label: &str,
    url: &str,
    x: &serde_json::Number,
    y: &serde_json::Number,
    direction: Direction,
    pixels: u16,
) -> String {
    let expected = serde_json::json!({"kind":kind,"label":label,"url":url,"x":x,"y":y});
    let expected = serde_json::to_string(&expected).expect("scroll observation serializes");
    let (left, top) = match direction {
        Direction::Up => (0, -(i32::from(pixels))),
        Direction::Down => (0, i32::from(pixels)),
        Direction::Left => (-(i32::from(pixels)), 0),
        Direction::Right => (i32::from(pixels), 0),
    };
    function(&format!(
        "const current = currentScroller(root, {slot}, {expected}); if (!current) return false; current.element.scrollBy({{left:{left},top:{top},behavior:'instant'}}); return {{x:current.element.scrollLeft,y:current.element.scrollTop}};"
    ))
}

pub(in crate::browser_worker::engine) fn verify(
    slot: usize,
    kind: &str,
    label: &str,
    url: &str,
    before_x: &serde_json::Number,
    before_y: &serde_json::Number,
    after_x: &serde_json::Number,
    after_y: &serde_json::Number,
) -> String {
    let expected =
        serde_json::json!({"kind":kind,"label":label,"url":url,"x":before_x,"y":before_y});
    let expected = serde_json::to_string(&expected).expect("scroll observation serializes");
    let after = serde_json::json!({"x":after_x,"y":after_y});
    let after = serde_json::to_string(&after).expect("scroll receipt serializes");
    function(&format!(
        "const current = currentScroller(root, {slot}, {expected}, false); return current && current.element.scrollLeft === {after}.x && current.element.scrollTop === {after}.y ? {after} : null;"
    ))
}
