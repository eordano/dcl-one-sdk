use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};

use async_trait::async_trait;
use tokio::sync::Notify;

use crate::eip1654::Eip1654Validator;
use crate::error::AuthError;

pub const DEFAULT_SUCCESS_TTL: Duration = Duration::from_secs(3600);

pub const DEFAULT_FAILURE_TTL: Duration = Duration::from_secs(300);

pub const DEFAULT_MAX_ENTRIES: usize = 100_000;

#[derive(Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    contract_address: String,
    hash_digest: [u8; 32],
    sig_digest: [u8; 32],
}

impl CacheKey {
    fn new(contract_address: &str, hash: &[u8], signature: &[u8]) -> Self {
        use sha2::{Digest, Sha256};

        let hash_digest: [u8; 32] = Sha256::digest(hash).into();
        let sig_digest: [u8; 32] = Sha256::digest(signature).into();

        Self {
            contract_address: contract_address.to_lowercase(),
            hash_digest,
            sig_digest,
        }
    }
}

#[derive(Clone)]
struct CacheEntry {
    result: bool,
    created_at: Instant,
    ttl: Duration,
}

impl CacheEntry {
    fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.ttl
    }
}

pub struct ValidationCache {
    inner: Arc<dyn Eip1654Validator>,
    cache: RwLock<HashMap<CacheKey, CacheEntry>>,
    insertion_order: RwLock<Vec<CacheKey>>,
    in_flight: Mutex<HashMap<CacheKey, Arc<Notify>>>,
    success_ttl: Duration,
    failure_ttl: Duration,
    max_entries: usize,
}

impl ValidationCache {
    pub fn new(inner: Arc<dyn Eip1654Validator>) -> Self {
        Self::with_config(
            inner,
            DEFAULT_SUCCESS_TTL,
            DEFAULT_FAILURE_TTL,
            DEFAULT_MAX_ENTRIES,
        )
    }

    pub fn with_config(
        inner: Arc<dyn Eip1654Validator>,
        success_ttl: Duration,
        failure_ttl: Duration,
        max_entries: usize,
    ) -> Self {
        Self {
            inner,
            cache: RwLock::new(HashMap::new()),
            insertion_order: RwLock::new(Vec::new()),
            in_flight: Mutex::new(HashMap::new()),
            success_ttl,
            failure_ttl,
            max_entries,
        }
    }

    pub fn len(&self) -> usize {
        self.cache.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        self.cache.write().clear();
        self.insertion_order.write().clear();
        self.in_flight.lock().clear();
    }

    fn evict_oldest(&self) {
        let mut order = self.insertion_order.write();
        let mut map = self.cache.write();

        while let Some(key) = order.first() {
            if let Some(entry) = map.get(key) {
                if entry.is_expired() {
                    let key = order.remove(0);
                    map.remove(&key);
                    continue;
                }
            } else {
                order.remove(0);
                continue;
            }
            break;
        }

        if map.len() >= self.max_entries {
            if let Some(key) = order.first().cloned() {
                order.remove(0);
                map.remove(&key);
            }
        }
    }

    fn insert(&self, key: CacheKey, result: bool) {
        if self.cache.read().len() >= self.max_entries {
            self.evict_oldest();
        }

        let ttl = if result {
            self.success_ttl
        } else {
            self.failure_ttl
        };
        let entry = CacheEntry {
            result,
            created_at: Instant::now(),
            ttl,
        };

        self.cache.write().insert(key.clone(), entry);
        self.insertion_order.write().push(key);
    }
}

#[async_trait]
impl Eip1654Validator for ValidationCache {
    async fn validate_signature(
        &self,
        contract_address: &str,
        hash: &[u8],
        signature: &[u8],
    ) -> Result<bool, AuthError> {
        let key = CacheKey::new(contract_address, hash, signature);

        {
            let map = self.cache.read();
            if let Some(entry) = map.get(&key) {
                if !entry.is_expired() {
                    return Ok(entry.result);
                }
            }
        }

        {
            let existing_notify = {
                let in_flight = self.in_flight.lock();
                in_flight.get(&key).cloned()
            };
            if let Some(notify) = existing_notify {
                notify.notified().await;
                let map = self.cache.read();
                if let Some(entry) = map.get(&key) {
                    if !entry.is_expired() {
                        return Ok(entry.result);
                    }
                }
            }
        }

        let notify = Arc::new(Notify::new());
        {
            let mut in_flight = self.in_flight.lock();
            in_flight.insert(key.clone(), notify.clone());
        }

        let result = self
            .inner
            .validate_signature(contract_address, hash, signature)
            .await;

        if let Ok(true) = &result {
            self.insert(key.clone(), true);
        }

        {
            let mut in_flight = self.in_flight.lock();
            in_flight.remove(&key);
        }
        notify.notify_waiters();

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingValidator {
        calls: AtomicUsize,
    }

    impl CountingValidator {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl Eip1654Validator for CountingValidator {
        async fn validate_signature(
            &self,
            _contract_address: &str,
            _hash: &[u8],
            _signature: &[u8],
        ) -> Result<bool, AuthError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        }
    }

    struct FailingValidator;

    #[async_trait]
    impl Eip1654Validator for FailingValidator {
        async fn validate_signature(
            &self,
            _contract_address: &str,
            _hash: &[u8],
            _signature: &[u8],
        ) -> Result<bool, AuthError> {
            Err(AuthError::Eip1654ValidationFailed("RPC unavailable".into()))
        }
    }

    #[tokio::test]
    async fn cache_hit_avoids_inner_call() {
        let inner = Arc::new(CountingValidator::new());
        let cache = ValidationCache::new(inner.clone());

        let addr = "0xabc";
        let hash = b"hello";
        let sig = b"world";

        let r1 = cache.validate_signature(addr, hash, sig).await.unwrap();
        assert!(r1);
        assert_eq!(inner.call_count(), 1);

        let r2 = cache.validate_signature(addr, hash, sig).await.unwrap();
        assert!(r2);
        assert_eq!(inner.call_count(), 1);
    }

    #[tokio::test]
    async fn expired_entry_triggers_refresh() {
        let inner = Arc::new(CountingValidator::new());
        let cache = ValidationCache::with_config(
            inner.clone(),
            Duration::from_millis(0),
            Duration::from_millis(0),
            100,
        );

        let addr = "0xabc";
        let hash = b"h";
        let sig = b"s";

        cache.validate_signature(addr, hash, sig).await.unwrap();
        assert_eq!(inner.call_count(), 1);

        cache.validate_signature(addr, hash, sig).await.unwrap();
        assert_eq!(inner.call_count(), 2);
    }

    #[tokio::test]
    async fn eviction_on_overflow() {
        let inner = Arc::new(CountingValidator::new());
        let cache = ValidationCache::with_config(
            inner.clone(),
            DEFAULT_SUCCESS_TTL,
            DEFAULT_FAILURE_TTL,
            2,
        );

        for i in 0..3u8 {
            cache.validate_signature("0xabc", &[i], &[i]).await.unwrap();
        }

        assert!(cache.len() <= 2, "cache should not exceed max_entries");
    }

    #[tokio::test]
    async fn case_insensitive_address() {
        let inner = Arc::new(CountingValidator::new());
        let cache = ValidationCache::new(inner.clone());

        cache.validate_signature("0xABC", b"h", b"s").await.unwrap();
        assert_eq!(inner.call_count(), 1);

        cache.validate_signature("0xabc", b"h", b"s").await.unwrap();
        assert_eq!(inner.call_count(), 1);
    }

    #[tokio::test]
    async fn rpc_error_is_not_cached() {
        let inner: Arc<dyn Eip1654Validator> = Arc::new(FailingValidator);
        let cache = ValidationCache::new(inner);

        let result = cache.validate_signature("0xabc", b"h", b"s").await;
        assert!(result.is_err());

        assert!(cache.is_empty());
    }

    #[test]
    fn clear_empties_cache() {
        let inner: Arc<dyn Eip1654Validator> = Arc::new(CountingValidator::new());
        let cache = ValidationCache::new(inner);

        let key = CacheKey::new("0xabc", b"h", b"s");
        cache.insert(key, true);
        assert_eq!(cache.len(), 1);

        cache.clear();
        assert!(cache.is_empty());
    }

    struct SlowValidator {
        calls: AtomicUsize,
        delay: Duration,
    }

    impl SlowValidator {
        fn new(delay: Duration) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                delay,
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl Eip1654Validator for SlowValidator {
        async fn validate_signature(
            &self,
            _contract_address: &str,
            _hash: &[u8],
            _signature: &[u8],
        ) -> Result<bool, AuthError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            Ok(true)
        }
    }

    #[tokio::test]
    async fn concurrent_misses_coalesce_into_one_rpc_call() {
        let inner = Arc::new(SlowValidator::new(Duration::from_millis(100)));
        let cache = Arc::new(ValidationCache::new(inner.clone()));

        let addr = "0xabc";
        let hash = b"hello";
        let sig = b"world";

        let n = 10;
        let mut handles = Vec::with_capacity(n);
        for _ in 0..n {
            let cache = cache.clone();
            handles.push(tokio::spawn(async move {
                cache.validate_signature(addr, hash, sig).await
            }));
        }

        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.unwrap());
        }

        assert_eq!(
            inner.call_count(),
            1,
            "expected 1 RPC call, got {}",
            inner.call_count()
        );
    }
}
