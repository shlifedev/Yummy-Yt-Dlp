use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::sync::{watch, Notify};

use crate::ytdlp::security;

pub struct DownloadManager {
    active_count: AtomicU32,
    max_concurrent: AtomicU32,
    /// task_id -> (attempt generation, cancel sender). The generation tags which executor
    /// attempt owns the entry: a stale attempt terminating late (cancel kill + stream draining
    /// can take seconds) must never drop a newer attempt's sender — the receiver side treats a
    /// sender drop as a cancel signal.
    cancel_senders: Mutex<HashMap<u64, (u64, watch::Sender<bool>)>>,
    cancel_generation: AtomicU64,
    idle_notify: Notify,
    shutting_down: AtomicBool,
}

impl DownloadManager {
    pub fn new(max_concurrent: u32) -> Self {
        Self {
            active_count: AtomicU32::new(0),
            max_concurrent: AtomicU32::new(security::clamp_max_concurrent(max_concurrent)),
            cancel_senders: Mutex::new(HashMap::new()),
            cancel_generation: AtomicU64::new(0),
            idle_notify: Notify::new(),
            shutting_down: AtomicBool::new(false),
        }
    }

    /// Mark the manager as draining for app shutdown: `try_acquire` stops handing out
    /// slots, so a cancelled executor's `process_next_pending` cannot claim the next
    /// pending row and spawn a fresh yt-dlp while exit paths wait for the active ones
    /// to be killed. One-way by design — `cancel_all` alone stays reusable for the
    /// reset/clear flows that keep the app (and queue) running afterwards.
    pub fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
    }

    pub fn active_count(&self) -> u32 {
        self.active_count.load(Ordering::SeqCst)
    }

    pub fn max_concurrent(&self) -> u32 {
        self.max_concurrent.load(Ordering::SeqCst)
    }

    /// Update the concurrency limit, clamped to a safe range
    /// (see `security::clamp_max_concurrent`) to prevent resource exhaustion.
    ///
    /// Lowering the limit below the number of in-flight downloads is intentional and does
    /// NOT reclaim the excess slots: running downloads keep their slot and finish naturally.
    /// Only *new* acquisitions are blocked until `active_count` falls back under the new
    /// limit, so the queue drains down to the new ceiling on its own.
    pub fn set_max_concurrent(&self, val: u32) {
        self.max_concurrent
            .store(security::clamp_max_concurrent(val), Ordering::SeqCst);
    }

    // CAS loop to fix TOCTOU race condition
    pub fn try_acquire(&self) -> bool {
        if self.shutting_down.load(Ordering::SeqCst) {
            return false;
        }
        loop {
            let current = self.active_count.load(Ordering::SeqCst);
            if current >= self.max_concurrent.load(Ordering::SeqCst) {
                return false;
            }
            if self
                .active_count
                .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return true;
            }
        }
    }

    pub fn release(&self) {
        let previous =
            self.active_count
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                    Some(count.saturating_sub(1))
                });

        if matches!(previous, Ok(1)) {
            self.idle_notify.notify_waiters();
        }
    }

    pub async fn wait_until_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;

        // enable() registers this waiter before we read active_count, so a release() that drops the
        // count to 0 between the check and the .await can't slip its notify_waiters() past us.
        let notified = self.idle_notify.notified();
        tokio::pin!(notified);

        loop {
            notified.as_mut().enable();

            if self.active_count() == 0 {
                return true;
            }

            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };

            if tokio::time::timeout(remaining, notified.as_mut())
                .await
                .is_err()
            {
                return self.active_count() == 0;
            }

            notified.set(self.idle_notify.notified());
        }
    }

    // Cancel support methods

    /// Register a cancel channel for a new executor attempt. Returns the attempt generation
    /// alongside the receiver; the executor must pass the same generation back to
    /// `unregister_cancel` so a stale attempt cannot remove a newer attempt's entry.
    /// An insert here can only overwrite an existing entry if two executors are ever live for
    /// the same task — the atomic claim paths prevent that; the overwritten sender's drop would
    /// read as a cancel on the older attempt, which is the safe direction.
    pub(super) fn register_cancel(&self, task_id: u64) -> (u64, watch::Receiver<bool>) {
        let generation = self.cancel_generation.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = watch::channel(false);
        let mut senders = self
            .cancel_senders
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        senders.insert(task_id, (generation, tx));
        (generation, rx)
    }

    /// Signal cancellation WITHOUT removing the sender: a registered entry marks the executor
    /// as still in flight (see `is_executing`), which retry_download uses to avoid racing an
    /// executor that is winding down. Cleanup happens via the generation-checked
    /// `unregister_cancel` when the attempt terminates.
    pub fn send_cancel(&self, task_id: u64) {
        let senders = self
            .cancel_senders
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some((_, tx)) = senders.get(&task_id) {
            let _ = tx.send(true);
        }
    }

    /// Whether an executor attempt for this task still holds its cancel channel — i.e. it is
    /// running, or winding down after a cancel (kill + stream draining can take seconds after
    /// the DB row already reads 'cancelled').
    pub fn is_executing(&self, task_id: u64) -> bool {
        let senders = self
            .cancel_senders
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        senders.contains_key(&task_id)
    }

    pub(super) fn unregister_cancel(&self, task_id: u64, generation: u64) {
        let mut senders = self
            .cancel_senders
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if matches!(senders.get(&task_id), Some((g, _)) if *g == generation) {
            senders.remove(&task_id);
        }
    }

    /// Remove a task's cancel sender regardless of generation. Only for the panic finalizer,
    /// where the panicked attempt's generation is unknowable; leaving the entry behind would
    /// make `is_executing` report the task as in flight for the rest of the session.
    pub(super) fn force_unregister_cancel(&self, task_id: u64) {
        let mut senders = self
            .cancel_senders
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        senders.remove(&task_id);
    }

    /// 앱 종료 시 모든 활성 다운로드 취소. 동기적으로 cancel signal만 전송.
    /// Senders stay registered (same semantics as `send_cancel`); each executor removes its
    /// own entry via the generation-checked `unregister_cancel` as it terminates.
    pub fn cancel_all(&self) {
        let senders = self
            .cancel_senders
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for (_generation, tx) in senders.values() {
            let _ = tx.send(true);
        }
    }
}
