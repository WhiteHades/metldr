#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Theme {
    #[default]
    Catppuccin,
    Gruvbox,
    Cyberpunk,
    Light,
}

impl Theme {
    pub(super) const ALL: [Self; 4] = [
        Self::Catppuccin,
        Self::Gruvbox,
        Self::Cyberpunk,
        Self::Light,
    ];

    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Catppuccin => "catppuccin",
            Self::Gruvbox => "gruvbox",
            Self::Cyberpunk => "cyberpunk",
            Self::Light => "light",
        }
    }

    pub(super) fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|theme| theme.key() == value)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum TextSize {
    #[default]
    Medium,
    Small,
    Large,
}

impl TextSize {
    pub(super) const ALL: [Self; 3] = [Self::Medium, Self::Small, Self::Large];

    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Medium => "medium",
            Self::Small => "small",
            Self::Large => "large",
        }
    }

    pub(super) fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|size| size.key() == value)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Values {
    pub(super) theme: Theme,
    pub(super) size: TextSize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Change {
    Theme(Theme),
    Size(TextSize),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Patch {
    pub(super) theme: Option<Theme>,
    pub(super) size: Option<TextSize>,
}

impl Patch {
    pub(super) fn empty(self) -> bool {
        self.theme.is_none() && self.size.is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Loading([bool; 2]),
    Ready,
    Saving(Patch),
    LoadFailed,
    SaveFailed(Patch),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Appearance {
    pub(super) values: Values,
    pub(super) phase: Phase,
    known: [bool; 2],
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            values: Values::default(),
            phase: Phase::Loading([false; 2]),
            known: [false; 2],
        }
    }
}

impl Appearance {
    pub(super) fn busy(&self) -> bool {
        matches!(self.phase, Phase::Loading(_) | Phase::Saving(_))
    }

    fn settled(&self) -> Phase {
        if self.known.into_iter().all(|known| known) {
            Phase::Ready
        } else {
            Phase::LoadFailed
        }
    }

    pub(super) fn loaded(&mut self, stored: Option<Values>) {
        let Phase::Loading(observed) = self.phase else {
            return;
        };
        if let Some(values) = stored {
            if !observed[0] {
                self.values.theme = values.theme;
            }
            if !observed[1] {
                self.values.size = values.size;
            }
            self.known = [true; 2];
            self.phase = Phase::Ready;
        } else {
            self.phase = self.settled();
        }
    }

    pub(super) fn external(&mut self, theme: Option<Theme>, size: Option<TextSize>) {
        let changed = [theme.is_some(), size.is_some()];
        if let Some(theme) = theme {
            self.values.theme = theme;
            self.known[0] = true;
        }
        if let Some(size) = size {
            self.values.size = size;
            self.known[1] = true;
        }
        match &mut self.phase {
            Phase::Loading(observed) => {
                for index in 0..2 {
                    observed[index] |= changed[index];
                }
            }
            Phase::Saving(pending) | Phase::SaveFailed(pending) => {
                if theme.is_some() {
                    pending.theme = None;
                }
                if size.is_some() {
                    pending.size = None;
                }
                if matches!(self.phase, Phase::SaveFailed(patch) if patch.empty()) {
                    self.phase = self.settled();
                }
            }
            Phase::Ready | Phase::LoadFailed => self.phase = self.settled(),
        }
    }

    pub(super) fn change(&mut self, change: Change) -> Result<Patch, &'static str> {
        if self.busy() {
            return Err("Appearance is busy. Try again shortly.");
        }
        let mut pending = match self.phase {
            Phase::SaveFailed(patch) => patch,
            _ => Patch::default(),
        };
        match change {
            Change::Theme(theme) => {
                self.values.theme = theme;
                self.known[0] = false;
                pending.theme = Some(theme);
            }
            Change::Size(size) => {
                self.values.size = size;
                self.known[1] = false;
                pending.size = Some(size);
            }
        }
        self.phase = Phase::Saving(pending);
        Ok(pending)
    }

    pub(super) fn saved(&mut self, success: bool) {
        let Phase::Saving(pending) = self.phase else {
            return;
        };
        if success {
            self.known[0] |= pending.theme.is_some();
            self.known[1] |= pending.size.is_some();
            self.phase = self.settled();
        } else if pending.empty() {
            self.phase = self.settled();
        } else {
            self.phase = Phase::SaveFailed(pending);
        }
    }

    pub(super) fn retry(&mut self) -> Result<Option<Patch>, &'static str> {
        if self.busy() {
            return Err("Appearance is busy. Try again shortly.");
        }
        match self.phase {
            Phase::SaveFailed(patch) => {
                self.phase = Phase::Saving(patch);
                Ok(Some(patch))
            }
            _ => {
                self.phase = Phase::Loading([false; 2]);
                Ok(None)
            }
        }
    }
}

pub(super) fn sounds_enabled(value: Option<&str>) -> bool {
    value != Some("off")
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn external_values_win_over_old_load_and_failed_save() {
        let mut appearance = Appearance::default();
        appearance.external(Some(Theme::Light), None);
        appearance.loaded(Some(Values {
            theme: Theme::Gruvbox,
            size: TextSize::Large,
        }));
        assert_eq!(
            appearance.values,
            Values {
                theme: Theme::Light,
                size: TextSize::Large
            }
        );
        let patch = appearance.change(Change::Theme(Theme::Cyberpunk)).unwrap();
        assert_eq!(patch.theme, Some(Theme::Cyberpunk));
        assert!(appearance.change(Change::Size(TextSize::Small)).is_err());
        appearance.external(Some(Theme::Gruvbox), None);
        appearance.saved(false);
        assert_eq!(appearance.values.theme, Theme::Gruvbox);
        assert_eq!(appearance.phase, Phase::Ready);
    }

    #[test]
    fn failed_preview_retries_only_dirty_values_without_claiming_unknown_size() {
        let mut appearance = Appearance::default();
        appearance.loaded(None);
        let patch = appearance.change(Change::Theme(Theme::Light)).unwrap();
        appearance.saved(false);
        assert_eq!(appearance.values.theme, Theme::Light);
        assert_eq!(appearance.retry().unwrap(), Some(patch));
        appearance.saved(true);
        assert_eq!(appearance.phase, Phase::LoadFailed);
        assert_eq!(appearance.retry().unwrap(), None);
        appearance.loaded(Some(Values {
            theme: Theme::Light,
            size: TextSize::Large,
        }));
        assert_eq!(appearance.phase, Phase::Ready);
        assert_eq!(appearance.values.size, TextSize::Large);
        assert!(!sounds_enabled(Some("off")));
        assert!(sounds_enabled(None));
        assert!(sounds_enabled(Some("false")));
        assert!(Theme::parse("dracula").is_none());
        assert_eq!(TextSize::parse("medium"), Some(TextSize::Medium));
    }
}
