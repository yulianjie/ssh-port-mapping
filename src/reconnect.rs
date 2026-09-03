use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// A connection that survives this long starts a fresh retry budget after its next disconnect.
pub const STABLE_CONNECTION_TIME: Duration = Duration::from_secs(30);
pub const MAX_RETRY_DELAY: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone)]
pub struct PendingRetry {
    pub attempt: u16,
    pub due_at: Instant,
    pub delay: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    Scheduled { attempt: u16, delay: Duration },
    Exhausted { attempts: u16 },
}

#[derive(Debug, Default)]
pub struct ReconnectTracker {
    pending: HashMap<Uuid, PendingRetry>,
    active_attempts: HashMap<Uuid, u16>,
}

impl ReconnectTracker {
    pub fn cancel(&mut self, id: Uuid) {
        self.pending.remove(&id);
        self.active_attempts.remove(&id);
    }

    pub fn mark_started(&mut self, id: Uuid, retry_attempt: Option<u16>) {
        self.pending.remove(&id);
        match retry_attempt {
            Some(attempt) => {
                self.active_attempts.insert(id, attempt);
            }
            None => {
                self.active_attempts.remove(&id);
            }
        }
    }

    pub fn after_exit(
        &mut self,
        id: Uuid,
        max_attempts: u16,
        initial_interval: Duration,
        runtime: Duration,
        now: Instant,
    ) -> RetryDecision {
        let completed_attempts = self.active_attempts.remove(&id).unwrap_or(0);
        let completed_attempts = if runtime >= STABLE_CONNECTION_TIME {
            0
        } else {
            completed_attempts
        };
        self.after_failure(id, completed_attempts, max_attempts, initial_interval, now)
    }

    pub fn after_start_failure(
        &mut self,
        id: Uuid,
        failed_attempt: u16,
        max_attempts: u16,
        initial_interval: Duration,
        now: Instant,
    ) -> RetryDecision {
        self.active_attempts.remove(&id);
        self.after_failure(id, failed_attempt, max_attempts, initial_interval, now)
    }

    pub fn take_due(&mut self, now: Instant) -> Vec<(Uuid, u16)> {
        let mut due: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, retry)| retry.due_at <= now)
            .map(|(id, retry)| (*id, retry.attempt))
            .collect();
        due.sort_unstable_by_key(|(id, _)| *id);
        for (id, _) in &due {
            self.pending.remove(id);
        }
        due
    }

    pub fn pending(&self, id: Uuid) -> Option<&PendingRetry> {
        self.pending.get(&id)
    }

    pub fn active_attempt(&self, id: Uuid) -> Option<u16> {
        self.active_attempts.get(&id).copied()
    }

    pub fn contains(&self, id: Uuid) -> bool {
        self.pending.contains_key(&id) || self.active_attempts.contains_key(&id)
    }

    fn after_failure(
        &mut self,
        id: Uuid,
        completed_attempts: u16,
        max_attempts: u16,
        initial_interval: Duration,
        now: Instant,
    ) -> RetryDecision {
        let next_attempt = completed_attempts.saturating_add(1);
        if next_attempt > max_attempts {
            self.pending.remove(&id);
            RetryDecision::Exhausted {
                attempts: completed_attempts,
            }
        } else {
            let delay = retry_delay(initial_interval, next_attempt);
            self.pending.insert(
                id,
                PendingRetry {
                    attempt: next_attempt,
                    due_at: now + delay,
                    delay,
                },
            );
            RetryDecision::Scheduled {
                attempt: next_attempt,
                delay,
            }
        }
    }
}

pub fn retry_delay(initial_interval: Duration, attempt: u16) -> Duration {
    let exponent = u32::from(attempt.saturating_sub(1));
    let multiplier = 1_u64.checked_shl(exponent).unwrap_or(u64::MAX);
    let seconds = initial_interval
        .as_secs()
        .max(1)
        .saturating_mul(multiplier)
        .min(MAX_RETRY_DELAY.as_secs());
    Duration::from_secs(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedules_only_when_the_retry_is_due() {
        let mut tracker = ReconnectTracker::default();
        let id = Uuid::new_v4();
        let now = Instant::now();

        assert_eq!(
            tracker.after_exit(id, 3, Duration::from_secs(5), Duration::from_secs(2), now,),
            RetryDecision::Scheduled {
                attempt: 1,
                delay: Duration::from_secs(5)
            }
        );
        assert!(tracker.take_due(now + Duration::from_secs(4)).is_empty());
        assert_eq!(
            tracker.take_due(now + Duration::from_secs(5)),
            vec![(id, 1)]
        );
    }

    #[test]
    fn exhausts_the_configured_attempt_budget() {
        let mut tracker = ReconnectTracker::default();
        let id = Uuid::new_v4();
        let now = Instant::now();

        tracker.mark_started(id, Some(3));
        assert_eq!(
            tracker.after_exit(id, 3, Duration::from_secs(5), Duration::from_secs(1), now,),
            RetryDecision::Exhausted { attempts: 3 }
        );
        assert!(tracker.pending(id).is_none());
    }

    #[test]
    fn failed_process_spawns_consume_retry_attempts() {
        let mut tracker = ReconnectTracker::default();
        let id = Uuid::new_v4();
        let now = Instant::now();
        let interval = Duration::from_secs(5);

        assert_eq!(
            tracker.after_start_failure(id, 0, 2, interval, now),
            RetryDecision::Scheduled {
                attempt: 1,
                delay: interval
            }
        );
        assert_eq!(
            tracker.after_start_failure(id, 1, 2, interval, now),
            RetryDecision::Scheduled {
                attempt: 2,
                delay: Duration::from_secs(10)
            }
        );
        assert_eq!(
            tracker.after_start_failure(id, 2, 2, interval, now),
            RetryDecision::Exhausted { attempts: 2 }
        );
    }

    #[test]
    fn stable_connection_receives_a_fresh_retry_budget() {
        let mut tracker = ReconnectTracker::default();
        let id = Uuid::new_v4();
        let now = Instant::now();

        tracker.mark_started(id, Some(3));
        assert_eq!(
            tracker.after_exit(id, 3, Duration::from_secs(5), STABLE_CONNECTION_TIME, now,),
            RetryDecision::Scheduled {
                attempt: 1,
                delay: Duration::from_secs(5)
            }
        );
    }

    #[test]
    fn cancelling_removes_pending_and_active_retry_state() {
        let mut tracker = ReconnectTracker::default();
        let id = Uuid::new_v4();
        let now = Instant::now();

        tracker.after_start_failure(id, 0, 3, Duration::from_secs(5), now);
        assert!(tracker.contains(id));
        tracker.cancel(id);
        assert!(!tracker.contains(id));

        tracker.mark_started(id, Some(1));
        assert!(tracker.contains(id));
        tracker.cancel(id);
        assert!(!tracker.contains(id));
    }

    #[test]
    fn retry_delay_doubles_until_the_five_minute_cap() {
        let initial = Duration::from_secs(5);
        let delays: Vec<_> = (1..=10)
            .map(|attempt| retry_delay(initial, attempt).as_secs())
            .collect();

        assert_eq!(delays, vec![5, 10, 20, 40, 80, 160, 300, 300, 300, 300]);
    }
}
