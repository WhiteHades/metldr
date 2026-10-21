use crate::Result;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct StopWait(Option<Instant>);

impl StopWait {
    pub(crate) fn check(&mut self, stopping: bool, now: Instant) -> Result<()> {
        if stopping {
            self.0.get_or_insert(now + Duration::from_secs(15));
        }
        if self.0.is_some_and(|deadline| now >= deadline) {
            return Err("Native agent did not confirm Stop within 15 seconds".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_stop_starts_the_confirmation_deadline() {
        let mut wait = StopWait::default();
        let now = Instant::now();
        let later = now + Duration::from_secs(86_400);
        wait.check(false, now).unwrap();
        wait.check(false, later).unwrap();
        wait.check(true, later).unwrap();
        wait.check(true, later + Duration::from_secs(14)).unwrap();
        assert_eq!(
            wait.check(true, later + Duration::from_secs(15))
                .unwrap_err()
                .to_string(),
            "Native agent did not confirm Stop within 15 seconds"
        );
        assert!(wait.check(false, later + Duration::from_secs(16)).is_err());
    }
}
