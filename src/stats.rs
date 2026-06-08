use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteBucket {
    Gh,
    Raw,
    Avatar,
    Unpkg,
    Mirror,
}

#[derive(Debug, serde::Serialize)]
pub struct RouteStats {
    pub gh: u64,
    pub raw: u64,
    pub avatar: u64,
    pub unpkg: u64,
    pub mirror: u64,
}

#[derive(Debug, Default)]
struct StatsInner {
    gh: AtomicU64,
    raw: AtomicU64,
    avatar: AtomicU64,
    unpkg: AtomicU64,
    mirror: AtomicU64,
}

/// Cloneable handle to a shared set of per-route request counters.
///
/// Clones share the same underlying atomics through an `Arc`. This matters
/// because axum clones the whole `AppContext` (and therefore `Stats`) on every
/// request: if each clone owned independent atomics, every `bump()` would land
/// on a throwaway copy and `/stats` would always report zero.
#[derive(Debug, Clone, Default)]
pub struct Stats {
    inner: Arc<StatsInner>,
}

impl Stats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bump(&self, bucket: RouteBucket) {
        let counter = match bucket {
            RouteBucket::Gh => &self.inner.gh,
            RouteBucket::Raw => &self.inner.raw,
            RouteBucket::Avatar => &self.inner.avatar,
            RouteBucket::Unpkg => &self.inner.unpkg,
            RouteBucket::Mirror => &self.inner.mirror,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> RouteStats {
        RouteStats {
            gh: self.inner.gh.load(Ordering::Relaxed),
            raw: self.inner.raw.load(Ordering::Relaxed),
            avatar: self.inner.avatar.load(Ordering::Relaxed),
            unpkg: self.inner.unpkg.load(Ordering::Relaxed),
            mirror: self.inner.mirror.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bump_and_snapshot() {
        let stats = Stats::new();
        stats.bump(RouteBucket::Gh);
        stats.bump(RouteBucket::Gh);
        stats.bump(RouteBucket::Raw);
        let snap = stats.snapshot();
        assert_eq!(snap.gh, 2);
        assert_eq!(snap.raw, 1);
        assert_eq!(snap.avatar, 0);
    }

    #[test]
    fn test_clone_shares_counters() {
        // A clone must increment the SAME underlying counters — this is the
        // exact property the old per-clone-atomics implementation broke.
        let stats = Stats::new();
        let clone = stats.clone();
        clone.bump(RouteBucket::Mirror);
        clone.bump(RouteBucket::Mirror);
        // Observed through the original handle.
        assert_eq!(stats.snapshot().mirror, 2);
    }
}
