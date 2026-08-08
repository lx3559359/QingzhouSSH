use serde::{Deserialize, Serialize};

const EMIT_INTERVAL_MS: i64 = 100;
const EWMA_ALPHA: f64 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferPhase {
    Connecting,
    Transferring,
    Verifying,
    Finalizing,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgressSnapshot {
    pub phase: TransferPhase,
    pub transferred: u64,
    pub total: Option<u64>,
    pub percent: Option<f64>,
    pub bytes_per_second: Option<f64>,
    pub average_bytes_per_second: Option<f64>,
    pub eta_seconds: Option<u64>,
}

#[derive(Debug)]
pub struct TransferProgressTracker {
    total: Option<u64>,
    started_at_ms: i64,
    last_sample_at_ms: i64,
    last_sample_bytes: u64,
    last_emitted_at_ms: Option<i64>,
    transferred: u64,
    bytes_per_second: Option<f64>,
    average_bytes_per_second: Option<f64>,
    phase: TransferPhase,
}

impl TransferProgressTracker {
    pub fn new(total: Option<u64>, started_at_ms: i64) -> Self {
        Self {
            total,
            started_at_ms,
            last_sample_at_ms: started_at_ms,
            last_sample_bytes: 0,
            last_emitted_at_ms: None,
            transferred: 0,
            bytes_per_second: None,
            average_bytes_per_second: None,
            phase: TransferPhase::Transferring,
        }
    }

    pub fn sample(&mut self, transferred: u64, now_ms: i64) -> Option<ProgressSnapshot> {
        self.observe(transferred, now_ms);
        let completed = self
            .total
            .is_some_and(|total| total > 0 && self.transferred >= total);
        let due = self
            .last_emitted_at_ms
            .is_none_or(|last| now_ms.saturating_sub(last) >= EMIT_INTERVAL_MS);
        if !due && !completed {
            return None;
        }
        self.last_emitted_at_ms = Some(now_ms);
        Some(self.snapshot())
    }

    pub fn change_phase(&mut self, phase: TransferPhase, now_ms: i64) -> ProgressSnapshot {
        if phase == TransferPhase::Transferring && self.phase != TransferPhase::Transferring {
            self.started_at_ms = now_ms;
            self.last_sample_at_ms = now_ms;
            self.last_sample_bytes = self.transferred;
            self.bytes_per_second = None;
            self.average_bytes_per_second = None;
        }
        self.phase = phase;
        self.last_emitted_at_ms = Some(now_ms);
        self.snapshot()
    }

    fn observe(&mut self, transferred: u64, now_ms: i64) {
        let transferred = transferred.max(self.transferred);
        let elapsed_ms = now_ms.saturating_sub(self.last_sample_at_ms);
        let delta_bytes = transferred.saturating_sub(self.last_sample_bytes);
        if elapsed_ms > 0 && delta_bytes > 0 {
            let instantaneous = delta_bytes as f64 * 1_000.0 / elapsed_ms as f64;
            if instantaneous.is_finite() && instantaneous > 0.0 {
                self.bytes_per_second = Some(match self.bytes_per_second {
                    Some(previous) => EWMA_ALPHA * instantaneous + (1.0 - EWMA_ALPHA) * previous,
                    None => instantaneous,
                });
            }
        }
        self.transferred = transferred;
        let total_elapsed_ms = now_ms.saturating_sub(self.started_at_ms);
        if total_elapsed_ms > 0 && transferred > 0 {
            let average = transferred as f64 * 1_000.0 / total_elapsed_ms as f64;
            if average.is_finite() && average > 0.0 {
                self.average_bytes_per_second = Some(average);
            }
        }
        if now_ms >= self.last_sample_at_ms {
            self.last_sample_at_ms = now_ms;
            self.last_sample_bytes = transferred;
        }
    }

    fn snapshot(&self) -> ProgressSnapshot {
        let percent = self
            .total
            .filter(|total| *total > 0)
            .map(|total| (self.transferred as f64 / total as f64 * 100.0).clamp(0.0, 100.0));
        let eta_seconds = match (self.total, self.bytes_per_second) {
            (Some(total), Some(speed)) if speed.is_finite() && speed > 0.0 => {
                Some(((total.saturating_sub(self.transferred) as f64) / speed).ceil() as u64)
            }
            _ => None,
        };
        ProgressSnapshot {
            phase: self.phase,
            transferred: self.transferred,
            total: self.total,
            percent,
            bytes_per_second: self.bytes_per_second,
            average_bytes_per_second: self.average_bytes_per_second,
            eta_seconds,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TransferPhase, TransferProgressTracker};

    #[test]
    fn throttles_to_ten_hertz_and_always_emits_completion() {
        let mut tracker = TransferProgressTracker::new(Some(1_000), 0);

        assert!(tracker.sample(100, 10).is_some());
        assert!(tracker.sample(200, 50).is_none());
        assert!(tracker.sample(300, 110).is_some());
        assert!(tracker.sample(1_000, 111).is_some());
    }

    #[test]
    fn computes_finite_smoothed_speed_average_and_eta() {
        let mut tracker = TransferProgressTracker::new(Some(1_000), 0);

        let first = tracker.sample(0, 0).unwrap();
        assert_eq!(first.bytes_per_second, None);
        assert_eq!(first.average_bytes_per_second, None);
        assert_eq!(first.eta_seconds, None);

        let second = tracker.sample(200, 100).unwrap();
        assert_eq!(second.bytes_per_second, Some(2_000.0));
        assert_eq!(second.average_bytes_per_second, Some(2_000.0));
        assert_eq!(second.eta_seconds, Some(1));
        assert!(second.bytes_per_second.unwrap().is_finite());
    }

    #[test]
    fn handles_zero_total_and_never_moves_transferred_backwards() {
        let mut tracker = TransferProgressTracker::new(Some(0), 0);

        let first = tracker.sample(300, 10).unwrap();
        assert_eq!(first.percent, None);
        let second = tracker.sample(200, 110).unwrap();
        assert_eq!(second.transferred, 300);
        assert_eq!(second.percent, None);
    }

    #[test]
    fn phase_changes_always_emit_even_inside_the_throttle_window() {
        let mut tracker = TransferProgressTracker::new(Some(1_000), 0);
        tracker.sample(100, 10).unwrap();

        let verifying = tracker.change_phase(TransferPhase::Verifying, 20);
        let finalizing = tracker.change_phase(TransferPhase::Finalizing, 21);

        assert_eq!(verifying.phase, TransferPhase::Verifying);
        assert_eq!(finalizing.phase, TransferPhase::Finalizing);
        assert_eq!(finalizing.transferred, 100);
    }

    #[test]
    fn entering_transfer_phase_excludes_connection_or_verification_time_from_speed() {
        let mut tracker = TransferProgressTracker::new(Some(1_000), 0);
        tracker.change_phase(TransferPhase::Verifying, 10);
        tracker.change_phase(TransferPhase::Transferring, 1_000);

        let snapshot = tracker.sample(100, 1_100).unwrap();

        assert_eq!(snapshot.bytes_per_second, Some(1_000.0));
        assert_eq!(snapshot.average_bytes_per_second, Some(1_000.0));
    }

    #[test]
    fn phase_changes_keep_the_last_transfer_average_stable() {
        let mut tracker = TransferProgressTracker::new(Some(1_000), 0);
        let transfer = tracker.sample(500, 100).unwrap();

        let verifying = tracker.change_phase(TransferPhase::Verifying, 10_000);

        assert_eq!(
            verifying.average_bytes_per_second,
            transfer.average_bytes_per_second
        );
    }
}
