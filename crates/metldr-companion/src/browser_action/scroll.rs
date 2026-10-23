use serde::{Deserialize, Serialize};
use serde_json::Number;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl From<metldr_browser_tools::ScrollDirection> for Direction {
    fn from(direction: metldr_browser_tools::ScrollDirection) -> Self {
        use metldr_browser_tools::ScrollDirection;
        match direction {
            ScrollDirection::Up => Self::Up,
            ScrollDirection::Down => Self::Down,
            ScrollDirection::Left => Self::Left,
            ScrollDirection::Right => Self::Right,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ScrollKind {
    Page,
    Region,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Point {
    pub(crate) x: Number,
    pub(crate) y: Number,
}

impl Point {
    pub(crate) fn valid(&self) -> bool {
        [&self.x, &self.y].into_iter().all(|number| {
            number
                .as_f64()
                .is_some_and(|value| value.is_finite() && value.abs() <= 1_000_000_000.0)
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Scroll {
    pub(crate) handle: String,
    pub(crate) kind: ScrollKind,
    pub(crate) label: String,
    pub(crate) direction: Direction,
    pub(crate) pixels: u16,
    pub(crate) before: Point,
}

impl Scroll {
    pub(crate) fn valid(&self) -> bool {
        !self.handle.is_empty()
            && self.handle.len() <= 64
            && self
                .handle
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            && !self.label.trim().is_empty()
            && self.label.len() <= 128
            && !self.label.chars().any(char::is_control)
            && (1..=2000).contains(&self.pixels)
            && self.before.valid()
    }
}
