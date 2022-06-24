use anyhow::{anyhow, Context, Result};
use tokio::sync::mpsc::Sender;

pub struct Notification {
    pub progress: u32,
    pub text: String,
}

pub struct Notifier {
    chan: Sender<Notification>,
}

impl Notifier {
    pub async fn notify_status(&self, status: Notification) -> Result<()> {
        self.chan
            .send(status)
            .await
            .map_err(|_| anyhow!("Notification channel closed??"))
    }
}
