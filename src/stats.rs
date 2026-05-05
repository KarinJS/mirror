use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RouteStats {
    pub gh: u64,
    pub raw: u64,
    pub avatar: u64,
    pub unpkg: u64,
    pub mirror: u64,
    pub other: u64,
}

#[derive(Clone)]
pub struct Stats {
    inner: Arc<RwLock<RouteStats>>,
}

impl Stats {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(RouteStats::default())),
        }
    }

    pub async fn bump(&self, bucket: &str) {
        let mut stats = self.inner.write().await;
        match bucket {
            "gh" => stats.gh += 1,
            "raw" => stats.raw += 1,
            "avatar" => stats.avatar += 1,
            "unpkg" => stats.unpkg += 1,
            "mirror" => stats.mirror += 1,
            _ => stats.other += 1,
        }
    }

    pub async fn snapshot(&self) -> RouteStats {
        self.inner.read().await.clone()
    }
}
