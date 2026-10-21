use super::*;
use wasm_bindgen_futures::spawn_local;

#[derive(Clone)]
pub(super) struct Selected {
    tab: i32,
    window: i32,
    url: String,
}

pub(super) fn affects_selected(
    inner: &Connection,
    namespace: &str,
    event: &str,
    details: &JsValue,
) -> bool {
    let selected = inner.selected.borrow();
    if namespace == "windows" && event == "onFocusChanged" {
        let window = details.as_f64();
        return window != Some(-1.0)
            && selected
                .as_ref()
                .is_none_or(|selected| window != Some(f64::from(selected.window)));
    }
    if namespace == "tabs" && event == "onUpdated" {
        return selected
            .as_ref()
            .is_none_or(|selected| details.as_f64() == Some(f64::from(selected.tab)));
    }
    if namespace == "permissions" {
        if event != "onRemoved" {
            return false;
        }
        let Some(site) = selected
            .as_ref()
            .and_then(|selected| crate::site::permission(&selected.url).ok())
        else {
            return false;
        };
        return get(details, "origins").ok().is_some_and(|origins| {
            Array::from(&origins)
                .iter()
                .filter_map(|origin| origin.as_string())
                .any(|removed| crate::site::removed_site(&removed, &site))
        });
    }
    true
}

pub(super) fn refresh(inner: &Rc<Connection>) {
    inner.selected.replace(None);
    let epoch = inner.selection_epoch.get().wrapping_add(1);
    inner.selection_epoch.set(epoch);
    let weak = Rc::downgrade(inner);
    spawn_local(async move {
        let Some(inner) = weak.upgrade() else { return };
        let selected = async {
            let tab = inner.active_tab().await?;
            let id = get(&tab, "id")
                .map_err(js_error)?
                .as_f64()
                .filter(|id| *id >= 0.0 && *id <= f64::from(i32::MAX) && id.fract() == 0.0)
                .ok_or_else(|| FlowError::Host("Select a browser tab.".into()))?
                as i32;
            let url = string_property(&tab, "url").map_err(js_error)?;
            let window = get(&tab, "windowId")
                .map_err(js_error)?
                .as_f64()
                .filter(|id| *id >= 0.0 && *id <= f64::from(i32::MAX) && id.fract() == 0.0)
                .ok_or_else(|| {
                    FlowError::Host("The selected tab has no valid browser window.".into())
                })? as i32;
            validate_url(&url)?;
            Ok::<_, FlowError>(Selected {
                tab: id,
                window,
                url,
            })
        }
        .await;
        if inner.selection_epoch.get() == epoch {
            inner.selected.replace(selected.ok());
        }
    });
}

pub(super) fn selected(flow: &FlowClient) -> impl Future<Output = Result<Value>> + use<> {
    let prepared = (|| {
        let cached = flow.inner.selected.borrow().clone();
        let selected = cached.ok_or_else(|| {
            refresh(&flow.inner);
            FlowError::Host("The selected page is updating. Try Capture again.".into())
        })?;
        let epoch = flow.inner.selection_epoch.get();
        let site = crate::site::permission(&selected.url)?;
        let grant = json_js(&json!({"origins":[site]}))?;
        let value = call(
            &chrome_api("permissions").map_err(js_error)?,
            "request",
            &[grant.clone()],
        )
        .map_err(js_error)?;
        Ok::<_, FlowError>((selected, epoch, grant, promise(value).map_err(js_error)?))
    })();
    let flow = flow.clone();
    async move {
        let (selected, epoch, grant, permission) = prepared?;
        if permission.await.map_err(js_error)?.as_bool() != Some(true) {
            return Err(FlowError::Host(
                "Access to the selected site was not granted.".into(),
            ));
        }
        let current = flow.inner.active_tab().await?;
        if flow.inner.selection_epoch.get() != epoch
            || get(&current, "id").map_err(js_error)?.as_f64() != Some(f64::from(selected.tab))
            || string_property(&current, "url").map_err(js_error)? != selected.url
        {
            return Err(changed());
        }
        let page = flow.inner.capture_selected().await?;
        let granted = await_call(
            &chrome_api("permissions").map_err(js_error)?,
            "contains",
            &[grant],
        )
        .await
        .map_err(js_error)?
        .as_bool()
            == Some(true);
        if !granted
            || flow.inner.selection_epoch.get() != epoch
            || page["tab"] != selected.tab
            || page["url"] != selected.url
        {
            return Err(changed());
        }
        Ok(page)
    }
}

fn changed() -> FlowError {
    FlowError::Host(
        "The selected page or site permission changed during capture. Nothing was sent.".into(),
    )
}

pub(super) async fn check_access(url: &str) -> Result<()> {
    let grant = json_js(&json!({"origins":[crate::site::permission(url)?]}))?;
    let granted = await_call(
        &chrome_api("permissions").map_err(js_error)?,
        "contains",
        &[grant],
    )
    .await
    .map_err(js_error)?;
    if granted.as_bool() != Some(true) {
        return Err(FlowError::Host(
            "Access to the selected site was not granted.".into(),
        ));
    }
    Ok(())
}
