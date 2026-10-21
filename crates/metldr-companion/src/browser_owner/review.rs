use crate::Result;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::Instant;

#[derive(Clone)]
pub(crate) struct Budget(Arc<Mutex<Clock>>);

struct Clock {
    limit: Duration,
    spent: Duration,
    phase: Phase,
}

enum Phase {
    Working(Instant),
    Review(Instant),
}

impl Clock {
    fn elapsed(&self) -> Duration {
        self.spent
            + match self.phase {
                Phase::Working(since) => since.elapsed(),
                Phase::Review(_) => Duration::ZERO,
            }
    }
}

impl Budget {
    pub(crate) fn new(limit: Duration) -> Self {
        Self(Arc::new(Mutex::new(Clock {
            limit,
            spent: Duration::ZERO,
            phase: Phase::Working(Instant::now()),
        })))
    }

    pub(crate) fn expired(&self) -> bool {
        self.0
            .lock()
            .map_or(true, |clock| clock.elapsed() >= clock.limit)
    }

    pub(crate) fn review_elapsed(&self) -> Option<Duration> {
        let clock = self.0.lock().ok()?;
        match clock.phase {
            Phase::Review(since) => Some(since.elapsed()),
            Phase::Working(_) => None,
        }
    }

    pub(super) fn pause(&self) -> Result<Review> {
        let mut clock = self.0.lock().map_err(|_| "browser budget poisoned")?;
        let elapsed = clock.elapsed();
        if elapsed >= clock.limit || matches!(clock.phase, Phase::Review(_)) {
            return Err("browser execution budget unavailable".into());
        }
        clock.spent = elapsed;
        clock.phase = Phase::Review(Instant::now());
        Ok(Review(self.clone()))
    }
}

pub(super) struct Review(Budget);

impl Drop for Review {
    fn drop(&mut self) {
        if let Ok(mut clock) = self.0.0.lock() {
            clock.phase = Phase::Working(Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::advance;

    #[tokio::test(start_paused = true)]
    async fn review_preserves_the_remaining_work_budget_and_other_requests_expire() {
        let budget = Budget::new(Duration::from_secs(30));
        advance(Duration::from_secs(12)).await;
        let review = budget.pause().unwrap();
        let queued = Budget::new(Duration::from_secs(30));
        advance(Duration::from_secs(90)).await;
        assert!(budget.review_elapsed().is_some());
        assert!(!budget.expired());
        assert!(queued.expired());
        assert!(queued.pause().is_err());
        drop(review);
        assert!(budget.review_elapsed().is_none());
        advance(Duration::from_secs(17)).await;
        assert!(!budget.expired());
        advance(Duration::from_secs(1)).await;
        assert!(budget.expired());
        assert!(budget.pause().is_err());
    }
}
