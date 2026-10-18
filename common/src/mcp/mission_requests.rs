use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Result, anyhow};
use tokio::sync::oneshot;

use crate::missions::MissionResponse;

type Response<T> = std::result::Result<T, String>;

pub type MissionRequests = ResponseRequests<MissionResponse>;

pub struct ResponseRequests<T> {
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Response<T>>>>>,
}

impl<T> Clone for ResponseRequests<T> {
    fn clone(&self) -> Self {
        Self {
            pending: self.pending.clone(),
        }
    }
}

impl<T> Default for ResponseRequests<T> {
    fn default() -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<T> ResponseRequests<T> {
    pub fn register(&self) -> PendingResponse<T> {
        let id = uuid::Uuid::new_v4().to_string();
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().unwrap().insert(id.clone(), sender);
        PendingResponse {
            id,
            receiver,
            requests: self.clone(),
        }
    }

    pub fn complete(&self, id: &str, result: Response<T>) {
        if let Some(sender) = self.pending.lock().unwrap().remove(id) {
            let _ = sender.send(result);
        }
    }
}

pub struct PendingResponse<T> {
    pub id: String,
    receiver: oneshot::Receiver<Response<T>>,
    requests: ResponseRequests<T>,
}

impl<T> PendingResponse<T> {
    pub async fn receive(mut self, timeout: Duration) -> Result<T> {
        tokio::time::timeout(timeout, &mut self.receiver)
            .await
            .map_err(|_| {
                anyhow!("Request timed out; inspect service state before retrying mutations")
            })?
            .map_err(|_| anyhow!("Response channel closed"))?
            .map_err(anyhow::Error::msg)
    }
}

impl<T> Drop for PendingResponse<T> {
    fn drop(&mut self) {
        self.requests.pending.lock().unwrap().remove(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn replies_are_correlated_and_late_replies_are_ignored() {
        let requests = MissionRequests::default();
        let first = requests.register();
        let second = requests.register();
        requests.complete(&second.id, Err("second".into()));
        requests.complete(&first.id, Err("first".into()));
        assert_eq!(
            first
                .receive(Duration::from_secs(1))
                .await
                .unwrap_err()
                .to_string(),
            "first"
        );
        assert_eq!(
            second
                .receive(Duration::from_secs(1))
                .await
                .unwrap_err()
                .to_string(),
            "second"
        );
        requests.complete("unknown", Err("late".into()));
        assert!(requests.pending.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn timeout_and_cancellation_remove_pending_requests() {
        let requests = MissionRequests::default();
        let timed = requests.register();
        assert!(timed.receive(Duration::ZERO).await.is_err());
        assert!(requests.pending.lock().unwrap().is_empty());
        let cancelled = requests.register();
        assert!(
            tokio::time::timeout(
                Duration::from_millis(1),
                cancelled.receive(Duration::from_secs(60)),
            )
            .await
            .is_err()
        );
        assert!(requests.pending.lock().unwrap().is_empty());
    }
}
