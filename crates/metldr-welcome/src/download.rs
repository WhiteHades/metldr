use std::{cell::RefCell, rc::Rc};

use js_sys::{Date, Promise};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::AbortController;

use crate::{
    browser,
    controller::Shared,
    state::{Choice, Status},
};

pub fn start(app: Shared) -> Result<(), JsValue> {
    let api = browser::summarizer().ok_or_else(|| JsValue::from_str("Chrome AI unavailable"))?;
    let abort = AbortController::new()?;
    let generation = {
        let mut app = app.borrow_mut();
        if app.setup.choice != Choice::Chrome
            || app.setup.status != Status::Downloadable
            || app.setup.saving
            || app.download.is_some()
        {
            return Ok(());
        }
        app.download = Some(abort.clone());
        let generation = app.setup.begin(Status::Downloading);
        app.setup.detail = "Chrome is preparing the model. Progress appears when Chrome reports it. You can keep using setup.".into();
        app.paint();
        generation
    };
    let progress_app = app.clone();
    let progress = Closure::<dyn FnMut(JsValue)>::new(move |event| {
        let Some(loaded) = browser::get(&event, "loaded").ok().and_then(|v| v.as_f64()) else {
            return;
        };
        let mut app = progress_app.borrow_mut();
        if !app.alive || !app.setup.current(generation) {
            return;
        }
        app.setup.update_progress(loaded);
        if let Some(value) = app.setup.progress {
            app.setup.detail = format!("Chrome reports {:.0}% downloaded.", value * 100.0);
        }
        app.paint();
    });
    let target = Rc::new(RefCell::new(None));
    let monitor_target = target.clone();
    let progress_function = progress.as_ref().clone();
    let monitor = Closure::<dyn FnMut(JsValue)>::new(move |value| {
        if browser::call(
            &value,
            "addEventListener",
            &["downloadprogress".into(), progress_function.clone()],
        )
        .is_ok()
        {
            *monitor_target.borrow_mut() = Some(value);
        }
    });
    let options = browser::object(&[
        ("type", "key-points".into()),
        ("format", "plain-text".into()),
        ("length", "medium".into()),
        ("signal", abort.signal().into()),
        ("monitor", monitor.as_ref().clone()),
    ])?;
    // Invoke before yielding so Chrome receives the click's user activation.
    let pending = browser::call(&api, "create", &[options]);
    spawn_local(async move {
        let result = match pending {
            Ok(promise) => JsFuture::from(Promise::resolve(&promise))
                .await
                .and_then(|session| {
                    browser::call(&session, "destroy", &[])?;
                    Ok(())
                }),
            Err(error) => Err(error),
        };
        if let Some(target) = target.borrow_mut().take() {
            let _ = browser::call(
                &target,
                "removeEventListener",
                &["downloadprogress".into(), progress.as_ref().clone()],
            );
        }
        drop(monitor);
        drop(progress);
        {
            let mut app = app.borrow_mut();
            if !app.alive || !app.setup.current(generation) {
                return;
            }
            app.download = None;
            match result {
                Ok(()) => {
                    app.setup.status = Status::Ready;
                    app.setup.progress = Some(1.0);
                    app.setup.downloaded_at = Some(Date::now());
                    app.setup.detail =
                        "Chrome AI is ready. The setup session has been released.".into();
                }
                Err(_) => {
                    app.setup.status = Status::Error;
                    app.setup.detail =
                        "Chrome could not finish model setup. Check availability to retry.".into();
                    app.paint();
                    return;
                }
            }
            app.paint();
        }
        let result = match browser::object(&[
            ("chromeAIDownloadComplete", Date::now().into()),
            ("chromeAIStatus", "available".into()),
        ]) {
            Ok(settings) => browser::save_settings(&settings).await,
            Err(error) => Err(error),
        };
        let app = app.borrow();
        if result.is_err() && app.alive && app.setup.current(generation) {
            app.notice("The model is ready, but metldr could not save its status. Reopen the panel to check availability again.");
        }
    });
    Ok(())
}
