//! The chats' storage limits as the sandbox uses them: `mcbench-store`'s
//! [`Storage`] behind `mcbench_sandbox::Budget` (`storage.js` `refusal`,
//! `measure`, `freeBytes`, `DISK_RESERVE_BYTES`, `forget`). This crate sees
//! both, so the adapter is here. The store's calls are blocking (a lock, a
//! `du` of the chat's folders): they run on `spawn_blocking`.

use std::sync::Arc;

use async_trait::async_trait;
use mcbench_store::Storage;

/// [`Storage`] as the sandbox's budget.
pub struct StoreBudget(pub Arc<Storage>);

#[async_trait]
impl mcbench_sandbox::Budget for StoreBudget {
    async fn refusal(&self, chat: &str) -> Result<Option<String>, String> {
        let (storage, chat) = (self.0.clone(), chat.to_string());
        tokio::task::spawn_blocking(move || storage.refusal(&chat).map_err(|error| error.message))
            .await
            .expect("a storage reading does not panic")
    }

    async fn measure(&self, chat: &str) -> Result<(), String> {
        let (storage, chat) = (self.0.clone(), chat.to_string());
        tokio::task::spawn_blocking(move || storage.measure(&chat).wait())
            .await
            .expect("a measurement does not panic")
    }

    fn free_bytes(&self) -> Result<u64, String> {
        self.0
            .free_bytes()
            .map(|free| free.max(0) as u64)
            .map_err(|error| error.message)
    }

    fn disk_reserve_bytes(&self) -> u64 {
        self.0.limits().disk_reserve_bytes
    }

    fn forget(&self, chat: &str) {
        self.0.forget(chat);
    }
}
