#![allow(dead_code)]

use chrono::Utc;
use std::io;
use std::{collections::HashMap, sync::Arc, sync::Mutex};

#[derive(Clone)]
pub struct CacheModel<V> {
    pub name: String,
    inner: Arc<Mutex<Inner<V>>>,
}

struct Inner<V> {
    value: HashMap<String, V>,
    expire: HashMap<String, i64>,    // per-key expire in seconds
    stored_at: HashMap<String, i64>, // stored timestamp (seconds)
    default_expire: Option<i64>,
}

pub trait Cache<V> {
    fn get(&self, key: &str) -> Option<V>;
    fn set(&self, key: &str, value: V);
    fn delete(&self, key: &str);
    fn expire(&self, key: &str, expire: i64) -> Result<(), io::Error>;
}

impl<V> Cache<V> for CacheModel<V>
where
    V: Clone,
{
    fn get(&self, key: &str) -> Option<V> {
        // Lock the single mutex once to avoid deadlocks caused by locking multiple mutexes.
        let mut inner = self.inner.lock().unwrap();

        // If the key has a stored_at timestamp, check expiry.
        if let Some(&store_at) = inner.stored_at.get(key) {
            let now = Utc::now().timestamp();
            // check per-key expire first
            if let Some(&expire_sec) = inner.expire.get(key) {
                if now - store_at >= expire_sec {
                    // expired: remove all associated entries
                    inner.value.remove(key);
                    inner.stored_at.remove(key);
                    inner.expire.remove(key);
                    return None;
                }
            } else if let Some(default_expire) = inner.default_expire {
                if now - store_at >= default_expire {
                    inner.value.remove(key);
                    inner.stored_at.remove(key);
                    return None;
                }
            }
        }

        inner.value.get(key).cloned()
    }

    fn set(&self, key: &str, value: V) {
        let mut inner = self.inner.lock().unwrap();
        let now = Utc::now().timestamp();
        inner.value.insert(key.to_string(), value);
        inner.stored_at.insert(key.to_string(), now);
        // New set clears any explicit expire to mean "no expire" unless expire is set later.
        inner.expire.remove(key);
    }

    fn delete(&self, key: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.value.remove(key);
        inner.stored_at.remove(key);
        inner.expire.remove(key);
    }

    fn expire(&self, key: &str, expire: i64) -> Result<(), io::Error> {
        let mut inner = self.inner.lock().unwrap();
        if inner.value.contains_key(key) {
            inner.expire.insert(key.to_string(), expire);
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, "Key not found"))
        }
    }
}

impl<V> CacheModel<V> {
    pub fn new(name: &str) -> Self {
        CacheModel {
            name: name.to_string(),
            inner: Arc::new(Mutex::new(Inner {
                value: HashMap::new(),
                expire: HashMap::new(),
                stored_at: HashMap::new(),
                default_expire: None,
            })),
        }
    }

    pub fn new_with_default_expire(name: &str, default_expire: i64) -> Self {
        CacheModel {
            name: name.to_string(),
            inner: Arc::new(Mutex::new(Inner {
                value: HashMap::new(),
                expire: HashMap::new(),
                stored_at: HashMap::new(),
                default_expire: Some(default_expire),
            })),
        }
    }
}

// Implement Debug for CacheModel when V: Debug to allow printing for diagnostics.
impl<V: std::fmt::Debug> std::fmt::Debug for CacheModel<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.lock().unwrap();
        f.debug_struct("CacheModel")
            .field("name", &self.name)
            .field("value", &inner.value)
            .field("expire", &inner.expire)
            .field("stored_at", &inner.stored_at)
            .field("default_expire", &inner.default_expire)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{Cache, CacheModel};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_cache_key_expire() {
        let cache: CacheModel<String> = CacheModel::new("default");
        cache.set("test_key", "test_value".to_string());
        cache.expire("test_key", 2).unwrap(); // 设置过期时间为2秒

        // 立即获取，应该存在
        assert_eq!(cache.get("test_key"), Some("test_value".to_string()));

        // 等待3秒，超过过期时间
        thread::sleep(Duration::from_secs(3));

        // 再次获取，应该不存在
        assert_eq!(cache.get("test_key"), None);
    }

    #[test]
    fn test_default_expire() {
        let cache: CacheModel<String> = CacheModel::new_with_default_expire("d", 1);
        cache.set("k", "v".to_string());

        assert_eq!(cache.get("k"), Some("v".to_string()));
        thread::sleep(Duration::from_secs(2));
        assert_eq!(cache.get("k"), None);
    }

    #[test]
    fn test_delete_and_set() {
        let cache: CacheModel<String> = CacheModel::new("d2");
        cache.set("a", "1".to_string());
        assert_eq!(cache.get("a"), Some("1".to_string()));
        cache.delete("a");
        assert_eq!(cache.get("a"), None);
        cache.set("a", "2".to_string());
        assert_eq!(cache.get("a"), Some("2".to_string()));
    }
}
