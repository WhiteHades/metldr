use std::{cell::RefCell, rc::Rc};

use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::spawn_local;
use web_sys::{AbortController, Document, Element, Event, HtmlSelectElement};

use crate::{
    browser, dom, download, guide_view, scene, setup_view,
    state::{Choice, Panel, Setup, Status},
};

pub type Shared = Rc<RefCell<App>>;

pub struct App {
    pub document: Document,
    pub setup: Setup,
    pub download: Option<AbortController>,
    pub alive: bool,
    theme_changed: bool,
    theme_revision: u64,
}

impl App {
    pub fn paint(&self) {
        if dom::render(&self.document, &self.setup, self.download.is_some()).is_err() {
            self.notice(
                "Setup could not update its controls. Reload to try again. Help remains available.",
            );
        }
    }

    pub fn notice(&self, message: &str) {
        let _ = dom::text(&self.document, "boot-status", message);
    }

    pub fn cancel(&mut self) {
        self.setup.generation += 1;
        if let Some(controller) = self.download.take() {
            controller.abort();
        }
    }
}

pub fn mount() -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("Window unavailable"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("Document unavailable"))?;
    guide_view::mount(&document)?;
    setup_view::mount(&document)?;
    scene::mount(&document)?;
    let app = Rc::new(RefCell::new(App {
        document: document.clone(),
        setup: Setup::default(),
        download: None,
        alive: true,
        theme_changed: false,
        theme_revision: 0,
    }));
    let clicks = app.clone();
    let click = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
        let button = event
            .target()
            .and_then(|v| v.dyn_into::<Element>().ok())
            .and_then(|v| v.closest("button").ok().flatten());
        if let Some(button) = button.filter(|v| !v.has_attribute("disabled")) {
            if button.id() == "open-agent-panel" {
                if event.is_trusted() {
                    open_panel(clicks.clone());
                }
            } else {
                handle_click(clicks.clone(), &button.id());
            }
        }
    });
    document.add_event_listener_with_callback("click", click.as_ref().unchecked_ref())?;
    click.forget();
    let changes = app.clone();
    let change = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
        if let Some(select) = event
            .target()
            .and_then(|v| v.dyn_into::<HtmlSelectElement>().ok())
        {
            handle_change(changes.clone(), &select.id(), select.value());
        }
    });
    document.add_event_listener_with_callback("change", change.as_ref().unchecked_ref())?;
    change.forget();
    let leaving = app.clone();
    let pagehide = Closure::<dyn FnMut(Event)>::new(move |_| {
        let mut app = leaving.borrow_mut();
        app.alive = false;
        app.cancel();
        if matches!(app.setup.status, Status::Checking | Status::Downloading) {
            app.setup.status = Status::Idle;
            app.setup.progress = None;
            app.setup.detail =
                "The setup request stopped when you left this page. Check again to continue."
                    .into();
        }
    });
    window.add_event_listener_with_callback("pagehide", pagehide.as_ref().unchecked_ref())?;
    pagehide.forget();
    let returning = app.clone();
    let pageshow = Closure::<dyn FnMut(Event)>::new(move |_| {
        if !returning.borrow().alive {
            if let Some(window) = web_sys::window() {
                let _ = window.location().reload();
            }
        }
    });
    window.add_event_listener_with_callback("pageshow", pageshow.as_ref().unchecked_ref())?;
    pageshow.forget();
    app.borrow().paint();
    spawn_local(prepare_panel(app.clone()));
    spawn_local(load(app));
    Ok(())
}

async fn prepare_panel(app: Shared) {
    let result = browser::prepare_panel().await;
    let mut app = app.borrow_mut();
    if app.alive {
        app.setup.panel = result.map_or(Panel::Unavailable, Panel::Ready);
        app.paint();
    }
}

fn open_panel(app: Shared) {
    let mut current = app.borrow_mut();
    let Panel::Ready(tab) = current.setup.panel else {
        return;
    };
    if !current.alive
        || current.setup.saving
        || !matches!(current.setup.choice, Choice::Provider | Choice::Agent)
    {
        return;
    }
    // Start Chrome's call before yielding so it retains the trusted click.
    let result = browser::start_panel_open(tab);
    current.setup.panel = Panel::Opening;
    current.paint();
    drop(current);
    spawn_local(async move {
        let result = match result {
            Ok(promise) => wasm_bindgen_futures::JsFuture::from(promise)
                .await
                .map(|_| ()),
            Err(error) => Err(error),
        };
        let mut app = app.borrow_mut();
        if !app.alive {
            return;
        }
        app.setup.panel = Panel::Ready(tab);
        let notice = if result.is_ok() {
            "Panel opened. Choose Settings to continue."
        } else {
            "The panel could not open. Try again or press Alt+Shift+M in Chrome."
        };
        app.notice(notice);
        app.paint();
    });
}

async fn load(app: Shared) {
    let result = browser::load_settings().await;
    let mut app = app.borrow_mut();
    if !app.alive {
        return;
    }
    match result {
        Ok(settings) => {
            if app.setup.generation == 0 {
                if browser::get(&settings, "preferredProvider")
                    .ok()
                    .and_then(|v| v.as_string())
                    .as_deref()
                    == Some("ollama")
                {
                    app.setup.choose(Choice::Ollama);
                }
                if let Some(model) = browser::get(&settings, "selectedModel")
                    .ok()
                    .and_then(|v| v.as_string())
                    .filter(|v| !v.is_empty() && v.len() <= 512)
                {
                    app.setup.model = model;
                }
            }
            if !app.theme_changed {
                if let Some(theme) = browser::get(&settings, "theme")
                    .ok()
                    .and_then(|v| v.as_string())
                    .filter(|v| valid_theme(v))
                {
                    app.setup.theme = theme;
                }
            }
            app.notice("Choose a connection to get started.");
        }
        Err(_) => app.notice(
            "Saved settings could not be read. Choose your settings again or reload to retry.",
        ),
    }
    app.setup.loading = false;
    app.paint();
}

fn handle_click(app: Shared, id: &str) {
    let choice = match id {
        "choose-local" | "choose-chrome" => Some(Choice::Chrome),
        "choose-ollama" => Some(Choice::Ollama),
        "choose-provider" => Some(Choice::Provider),
        "choose-agent" => Some(Choice::Agent),
        _ => None,
    };
    if let Some(choice) = choice {
        let mut app = app.borrow_mut();
        if app.setup.saving {
            return;
        }
        app.cancel();
        app.setup.choose(choice);
        app.paint();
        return;
    }
    match id {
        "check-button" => check(app),
        "download-button" => {
            if download::start(app.clone()).is_err() {
                let mut app = app.borrow_mut();
                app.cancel();
                app.setup.status = Status::Error;
                app.setup.detail =
                    "The model download could not start. Check again to retry.".into();
                app.paint();
            }
        }
        "cancel-button" => {
            let mut app = app.borrow_mut();
            app.cancel();
            app.setup.status = Status::Idle;
            app.setup.progress = None;
            app.setup.detail = "The setup request was cancelled. Chrome may continue a model download shared with other pages.".into();
            app.paint();
        }
        "continue-button" => finish(app, true),
        "explore-button" => finish(app, false),
        _ => {}
    }
}

fn handle_change(app: Shared, id: &str, value: String) {
    let mut current = app.borrow_mut();
    if current.setup.saving {
        return;
    }
    match id {
        "model-choice" if current.setup.models.contains(&value) => current.setup.model = value,
        "theme-choice" if valid_theme(&value) => {
            current.theme_changed = true;
            current.theme_revision += 1;
            current.setup.theme = value;
            if !current.setup.theme_saving {
                current.setup.theme_saving = true;
                spawn_local(save_theme(app.clone()));
            }
        }
        _ => return,
    }
    current.paint();
}

async fn save_theme(app: Shared) {
    loop {
        let (theme, revision) = {
            let app = app.borrow();
            if !app.alive {
                return;
            }
            (app.setup.theme.clone(), app.theme_revision)
        };
        let result = match browser::object(&[("theme", theme.into())]) {
            Ok(settings) => browser::save_settings(&settings).await,
            Err(error) => Err(error),
        };
        let mut app = app.borrow_mut();
        if !app.alive {
            return;
        }
        if result.is_err() {
            app.notice(
                "The theme changed for this page but could not be saved. Try choosing it again.",
            );
        }
        if revision == app.theme_revision {
            app.setup.theme_saving = false;
            app.paint();
            return;
        }
    }
}

fn check(app: Shared) {
    let (generation, choice) = {
        let mut app = app.borrow_mut();
        if app.setup.saving || app.download.is_some() || app.setup.status == Status::Checking {
            return;
        }
        let choice = app.setup.choice;
        if !matches!(choice, Choice::Chrome | Choice::Ollama) {
            return;
        }
        let generation = app.setup.begin(Status::Checking);
        app.setup.detail = "Checking availability. No prompt or page content is sent.".into();
        app.paint();
        (generation, choice)
    };
    spawn_local(async move {
        let result = match choice {
            Choice::Chrome => {
                if let Some(api) = browser::summarizer() {
                    browser::await_call(&api, "availability", &[])
                        .await
                        .map(|v| (v.as_string(), Vec::new()))
                } else {
                    Ok((Some("missing-api".into()), Vec::new()))
                }
            }
            Choice::Ollama => browser::check_ollama().await.map(|models| (None, models)),
            _ => return,
        };
        let mut app = app.borrow_mut();
        if !app.alive || !app.setup.current(generation) {
            return;
        }
        match result {
            Ok((availability, _)) if choice == Choice::Chrome => app
                .setup
                .availability(availability.as_deref().unwrap_or("unknown")),
            Ok((_, models)) => {
                app.setup.models = models;
                if !app.setup.models.contains(&app.setup.model) {
                    app.setup.model = app.setup.models.first().cloned().unwrap_or_default();
                }
                app.setup.status = if app.setup.models.is_empty() {
                    Status::Unavailable
                } else {
                    Status::Ready
                };
                app.setup.detail = if app.setup.models.is_empty() {
                    "Ollama is reachable but has no installed models. Install a model in Ollama, then check again."
                } else { "Ollama is reachable. Choose an installed model before saving your settings." }.into();
                if dom::models(&app.document, &app.setup.models, &app.setup.model).is_err() {
                    app.setup.status = Status::Error;
                    app.setup.detail =
                        "Installed models could not be shown. Reload to try again.".into();
                }
            }
            Err(_) => {
                app.setup.status = Status::Error;
                app.setup.detail = match choice {
                    Choice::Chrome => "Chrome AI could not be checked. Try again or choose another connection.",
                    _ => "Ollama could not be checked at 127.0.0.1:11434. Start Ollama and allow this extension origin, then retry.",
                }.into();
            }
        }
        app.paint();
    });
}

fn finish(app: Shared, connected: bool) {
    let settings = {
        let mut app = app.borrow_mut();
        if app.setup.loading
            || app.setup.saving
            || app.setup.theme_saving
            || (connected && !app.setup.ready())
        {
            return;
        }
        let mut fields = vec![
            ("onboardingComplete", true.into()),
            ("theme", app.setup.theme.clone().into()),
        ];
        if let Some(time) = app.setup.downloaded_at {
            fields.push(("chromeAIDownloadComplete", time.into()));
            fields.push(("chromeAIStatus", "available".into()));
        }
        if connected {
            fields.push((
                "preferredProvider",
                match app.setup.choice {
                    Choice::Chrome => "chrome-ai",
                    _ => "ollama",
                }
                .into(),
            ));
            if app.setup.choice == Choice::Ollama {
                fields.push(("selectedModel", app.setup.model.clone().into()));
            }
        }
        let settings = browser::object(&fields);
        app.cancel();
        app.setup.saving = true;
        app.paint();
        settings
    };
    spawn_local(async move {
        let result = match settings {
            Ok(settings) => browser::save_settings(&settings).await,
            Err(error) => Err(error),
        };
        let mut app = app.borrow_mut();
        if !app.alive {
            return;
        }
        app.setup.saving = false;
        match result {
            Ok(()) => {
                app.notice("Setup saved. You can close this tab and open metldr from the toolbar.");
                if let Some(window) = web_sys::window() {
                    let _ = window.close();
                }
            }
            Err(_) => {
                app.notice("Setup could not be saved. This tab will stay open. Try saving again.")
            }
        }
        app.paint();
    });
}

fn valid_theme(value: &str) -> bool {
    matches!(value, "catppuccin" | "gruvbox" | "cyberpunk" | "light")
}
