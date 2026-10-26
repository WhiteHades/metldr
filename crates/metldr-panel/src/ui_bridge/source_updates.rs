use super::*;
use crate::model::BrowserActionPhase;
use wasm_bindgen_futures::spawn_local;

impl PageSession {
    pub(super) fn install_source_updates(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.flow.borrow().on_source_change(move || {
            if let Some(page) = weak.upgrade() {
                page.notify_settings();
                page.refresh_action_source();
            }
        });
    }

    pub(super) fn refresh_action_source(self: &Rc<Self>) {
        if self.invalidated.get()
            || (!self.has_source_observers()
                && !self
                    .view
                    .borrow()
                    .browser_actions
                    .iter()
                    .any(|action| action.phase == BrowserActionPhase::Pending))
            || self.source_refreshing.replace(true)
        {
            return;
        }
        let page = Rc::clone(self);
        spawn_local(async move {
            loop {
                if page.invalidated.get() || page.current.borrow().upgrade().is_none() {
                    break;
                }
                let flow = page.flow.borrow().clone();
                let view = page.view.borrow().clone();
                let epoch = flow.source_epoch();
                let scope = view.selection.clone();
                let checked = flow.check_source(view).await;
                if !flow.same_connection(&page.flow.borrow())
                    || page.view.borrow().selection != scope
                {
                    continue;
                }
                if page.accept(checked).is_err() {
                    break;
                }
                if flow.source_epoch() == epoch {
                    break;
                }
            }
            page.source_refreshing.set(false);
        });
    }
}
