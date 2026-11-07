use super::*;

struct Capturing(Rc<Core>);

impl Drop for Capturing {
    fn drop(&mut self) {
        let changed = self.0.owner.borrow_mut().capture_finished();
        if changed {
            self.0.page.notify_settings();
        }
    }
}

impl Core {
    pub(super) fn capture(
        self: &Rc<Self>,
    ) -> Result<impl Future<Output = Result<(), FlowError>> + 'static, FlowError> {
        self.ensure_current()?;
        let view = self.cached()?;
        if self.page.settings_blocked() {
            return Err(FlowError::HistoryBusy);
        }
        self.owner.borrow_mut().capture()?;
        let pending = Capturing(Rc::clone(self));
        // Chrome must receive the permission request before the trusted input handler yields.
        let capture = self.flow().capture_selected(&view);
        self.page.notify_settings();
        Ok(async move {
            capture
                .await
                .and_then(|view| pending.0.accept(view))
                .map(|_| ())
        })
    }
}
