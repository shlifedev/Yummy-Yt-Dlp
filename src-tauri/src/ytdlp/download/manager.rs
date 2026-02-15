use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use tokio::sync::watch;

pub struct DownloadManager {
    active_count: AtomicU32,
    max_concurrent: AtomicU32,
    cancel_senders: Mutex<HashMap<u64, watch::Sender<bool>>>,
}

impl DownloadManager {
    pub fn new(max_concurrent: u32) -> Self {
        Self {
            active_count: AtomicU32::new(0),
            max_concurrent: AtomicU32::new(max_concurrent.clamp(1, 20)),
            cancel_senders: Mutex::new(HashMap::new()),
        }
    }

    pub fn active_count(&self) -> u32 {
        self.active_count.load(Ordering::SeqCst)
    }

    pub fn max_concurrent(&self) -> u32 {
        self.max_concurrent.load(Ordering::SeqCst)
    }

    // Clamp max_concurrent to [1, 20] to prevent resource exhaustion
    pub fn set_max_concurrent(&self, val: u32) {
        self.max_concurrent
            .store(val.clamp(1, 20), Ordering::SeqCst);
    }

    // CAS loop to fix TOCTOU race condition
    pub fn try_acquire(&self) -> bool {
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
        let _ = self
            .active_count
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                Some(count.saturating_sub(1))
            });
    }

    /// Synchronize active_count with the actual DB state.
    /// Used after cancel_all to correct any drift between the atomic counter
    /// and the real number of downloading tasks.
    pub fn sync_active_count(&self, count: u32) {
        self.active_count.store(count, Ordering::SeqCst);
    }

    // Cancel support methods
    pub(super) fn register_cancel(&self, task_id: u64) -> watch::Receiver<bool> {
        let (tx, rx) = watch::channel(false);
        let mut senders = self
            .cancel_senders
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        senders.insert(task_id, tx);
        rx
    }

    pub fn send_cancel(&self, task_id: u64) {
        let mut senders = self
            .cancel_senders
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = senders.remove(&task_id) {
            let _ = tx.send(true);
        }
    }

    pub(super) fn unregister_cancel(&self, task_id: u64) {
        let mut senders = self
            .cancel_senders
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        senders.remove(&task_id);
    }

    /// 앱 종료 시 모든 활성 다운로드 취소. 동기적으로 cancel signal만 전송.
    pub fn cancel_all(&self) {
        let mut senders = self
            .cancel_senders
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for (_task_id, tx) in senders.drain() {
            let _ = tx.send(true);
        }
    }
}
