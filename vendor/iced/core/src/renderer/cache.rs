use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

/// An ownership token for a renderer's cached layer resources.
///
/// Clones share the same identity and ownership. Backends can keep a
/// [`Weak`] owner to release retained GPU resources after the last token is
/// dropped, without extending the token's lifetime.
#[derive(Debug, Clone)]
pub struct Cache {
    id: u64,
    owner: Arc<()>,
}

impl Cache {
    /// Creates a new cache token with a unique identity.
    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);

        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            owner: Arc::new(()),
        }
    }

    /// Returns the stable identity shared by this token and its clones.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Returns a weak owner for tracking the lifetime of cached GPU resources.
    ///
    /// The owner can be upgraded while at least one clone of this token remains.
    pub fn downgrade(&self) -> Weak<()> {
        Arc::downgrade(&self.owner)
    }
}

impl Default for Cache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::Cache;

    #[test]
    fn clones_share_identity_and_owner() {
        let cache = Cache::new();
        let clone = cache.clone();

        assert_eq!(cache.id(), clone.id());
        assert!(cache.downgrade().ptr_eq(&clone.downgrade()));
    }

    #[test]
    fn default_creates_distinct_tokens() {
        let first = Cache::default();
        let second = Cache::default();
        let third = Cache::new();

        assert_ne!(first.id(), second.id());
        assert_ne!(first.id(), third.id());
        assert_ne!(second.id(), third.id());
        assert!(!first.downgrade().ptr_eq(&second.downgrade()));
    }

    #[test]
    fn weak_owner_expires_after_the_last_clone_is_dropped() {
        let cache = Cache::new();
        let clone = cache.clone();
        let owner = cache.downgrade();

        drop(cache);
        assert!(owner.upgrade().is_some());

        drop(clone);
        assert!(owner.upgrade().is_none());
    }
}
