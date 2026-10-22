#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Choice {
    Chrome,
    Ollama,
    Provider,
    Agent,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Status {
    Idle,
    Checking,
    Ready,
    Downloadable,
    Downloading,
    Unavailable,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Panel {
    Preparing,
    Ready(i32),
    Opening,
    Unavailable,
}

pub struct Setup {
    pub panel: Panel,
    pub choice: Choice,
    pub status: Status,
    pub generation: u64,
    pub models: Vec<String>,
    pub model: String,
    pub progress: Option<f64>,
    pub detail: String,
    pub loading: bool,
    pub saving: bool,
    pub theme: String,
    pub theme_saving: bool,
    pub downloaded_at: Option<f64>,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            panel: Panel::Preparing,
            choice: Choice::Chrome,
            status: Status::Idle,
            generation: 0,
            models: Vec::new(),
            model: String::new(),
            progress: None,
            detail: "Check Chrome AI or choose another connection. No model has been contacted."
                .into(),
            loading: true,
            saving: false,
            theme: "catppuccin".into(),
            theme_saving: false,
            downloaded_at: None,
        }
    }
}

impl Setup {
    pub fn choose(&mut self, choice: Choice) {
        self.generation += 1;
        self.choice = choice;
        self.status = Status::Idle;
        self.progress = None;
        self.detail = match choice {
            Choice::Chrome => "Check whether Chrome AI is ready on this device.",
            Choice::Ollama => "Check the Ollama service at 127.0.0.1:11434. The check lists installed models and sends no prompt.",
            Choice::Provider => "In the panel, choose Settings, then Providers.",
            Choice::Agent => "In the panel, choose Settings, then Installed agents.",
        }.into();
    }

    pub fn begin(&mut self, status: Status) -> u64 {
        self.generation += 1;
        self.status = status;
        self.progress = None;
        self.generation
    }

    pub fn current(&self, generation: u64) -> bool {
        self.generation == generation
    }

    pub fn ready(&self) -> bool {
        self.status == Status::Ready
            && match self.choice {
                Choice::Chrome => true,
                Choice::Ollama => self.models.contains(&self.model),
                Choice::Provider | Choice::Agent => false,
            }
    }

    pub fn availability(&mut self, value: &str) {
        let (status, detail) = match value {
            "missing-api" => (
                Status::Unavailable,
                "Chrome does not expose the Summarizer API on this page. Check browser and device support in Help, or choose Ollama.",
            ),
            "available" => (Status::Ready, "Chrome AI is ready on this device."),
            "downloadable" => (
                Status::Downloadable,
                "Chrome can download the model. Download size depends on the browser and model version.",
            ),
            "downloading" => (
                Status::Downloading,
                "Chrome reports a download in progress. Check again later for its status.",
            ),
            "unavailable" => (
                Status::Unavailable,
                "Chrome AI is unavailable on this device. You can use Ollama or continue without connecting.",
            ),
            _ => (
                Status::Error,
                "Chrome returned an unknown availability state. Check again or choose another connection.",
            ),
        };
        self.status = status;
        self.detail = detail.into();
    }

    pub fn update_progress(&mut self, loaded: f64) {
        if loaded.is_finite() && (0.0..=1.0).contains(&loaded) {
            self.progress = Some(self.progress.unwrap_or(0.0).max(loaded));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_invalidate_checks_and_require_a_current_model() {
        let mut setup = Setup::default();
        let first = setup.begin(Status::Checking);
        setup.choose(Choice::Ollama);
        assert!(!setup.current(first));
        setup.status = Status::Ready;
        setup.models = vec!["model:latest".into()];
        setup.model = "deleted:latest".into();
        assert!(!setup.ready());
        setup.model = "model:latest".into();
        assert!(setup.ready());
        setup.choose(Choice::Provider);
        setup.panel = Panel::Ready(7);
        setup.status = Status::Ready;
        assert!(!setup.ready());
        setup.choose(Choice::Agent);
        assert!(!setup.ready());
        assert_eq!(setup.panel, Panel::Ready(7));
    }

    #[test]
    fn availability_and_fractional_progress_are_not_estimates() {
        let mut setup = Setup::default();
        setup.availability("downloadable");
        assert_eq!(setup.status, Status::Downloadable);
        assert_eq!(setup.progress, None);
        setup.availability("downloading");
        assert_eq!(setup.status, Status::Downloading);
        setup.update_progress(0.25);
        setup.update_progress(0.1);
        setup.update_progress(f64::NAN);
        setup.update_progress(50.0);
        assert_eq!(setup.progress, Some(0.25));
        setup.availability("available");
        assert!(setup.ready());
        setup.availability("unexpected");
        assert_eq!(setup.status, Status::Error);
    }
}
