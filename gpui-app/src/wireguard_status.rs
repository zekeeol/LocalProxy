//! Cached WireGuard snapshots and the requests that may update them.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use local_proxy_rust::api::local_proxy::WireGuardStatus;

pub struct StatusSnapshot {
    pub status: WireGuardStatus,
    failed: bool,
    updated_at: Instant,
}

impl StatusSnapshot {
    pub fn is_stale(&self, now: Instant, max_age: Duration) -> bool {
        self.failed || now.saturating_duration_since(self.updated_at) >= max_age
    }
}

#[derive(Default)]
pub struct StatusCache {
    snapshots: HashMap<String, StatusSnapshot>,
    pending: HashMap<String, u64>,
    next_request_id: u64,
}

impl StatusCache {
    pub fn get(&self, id: &str) -> Option<&StatusSnapshot> {
        self.snapshots.get(id)
    }

    pub fn is_pending(&self, id: &str) -> bool {
        self.pending.contains_key(id)
    }

    /// Deduplicate requests for the same profile while one is in flight.
    pub fn begin(&mut self, id: &str) -> Option<u64> {
        if self.is_pending(id) {
            return None;
        }
        self.next_request_id = self.next_request_id.wrapping_add(1);
        let request_id = self.next_request_id;
        self.pending.insert(id.to_string(), request_id);
        Some(request_id)
    }

    /// Ignore replies from an earlier tunnel lifecycle or superseded request.
    pub fn finish(
        &mut self,
        id: &str,
        request_id: u64,
        result: Result<Option<WireGuardStatus>, String>,
        now: Instant,
    ) -> bool {
        if self.pending.get(id) != Some(&request_id) {
            return false;
        }
        self.pending.remove(id);
        match result {
            Ok(Some(status)) => {
                self.snapshots.insert(
                    id.to_string(),
                    StatusSnapshot {
                        status,
                        failed: false,
                        updated_at: now,
                    },
                );
            }
            Ok(None) | Err(_) => {
                if let Some(snapshot) = self.snapshots.get_mut(id) {
                    snapshot.failed = true;
                }
            }
        }
        true
    }

    pub fn invalidate(&mut self, id: &str) {
        self.snapshots.remove(id);
        self.pending.remove(id);
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.snapshots.retain(|id, _| keep(id));
        self.pending.retain(|id, _| keep(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(interface: &str) -> WireGuardStatus {
        WireGuardStatus {
            interface_name: interface.into(),
            listen_port: 0,
            public_key: None,
            peers: Vec::new(),
            health_warning: None,
        }
    }

    #[test]
    fn keeps_each_profiles_last_snapshot_when_a_refresh_fails() {
        let mut cache = StatusCache::default();
        let now = Instant::now();
        let first = cache.begin("a").unwrap();
        assert!(cache.finish("a", first, Ok(Some(status("utun1"))), now));
        let second = cache.begin("b").unwrap();
        assert!(cache.finish("b", second, Ok(Some(status("utun2"))), now));

        let refresh = cache.begin("a").unwrap();
        assert!(cache.begin("a").is_none());
        assert_eq!(cache.get("a").unwrap().status.interface_name, "utun1");
        assert!(cache.finish("a", refresh, Ok(None), now));
        assert_eq!(cache.get("a").unwrap().status.interface_name, "utun1");
        assert!(cache
            .get("a")
            .unwrap()
            .is_stale(now, Duration::from_secs(10)));
        assert!(!cache
            .get("b")
            .unwrap()
            .is_stale(now, Duration::from_secs(10)));
        assert!(cache
            .get("b")
            .unwrap()
            .is_stale(now + Duration::from_secs(10), Duration::from_secs(10)));
    }

    #[test]
    fn ignores_late_results_after_stop_or_profile_removal() {
        let mut cache = StatusCache::default();
        let now = Instant::now();
        let old = cache.begin("a").unwrap();
        cache.invalidate("a");
        let current = cache.begin("a").unwrap();
        assert_ne!(old, current);
        assert!(!cache.finish("a", old, Ok(Some(status("old"))), now));
        assert!(cache.finish("a", current, Ok(Some(status("new"))), now));

        let pending = cache.begin("b").unwrap();
        cache.retain(|id| id == "a");
        assert!(!cache.finish("b", pending, Ok(Some(status("removed"))), now));
        assert_eq!(cache.get("a").unwrap().status.interface_name, "new");
        assert!(cache.get("b").is_none());
    }
}
