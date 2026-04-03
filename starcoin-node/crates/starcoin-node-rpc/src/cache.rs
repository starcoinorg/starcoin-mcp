use super::*;

#[derive(Debug)]
pub(crate) struct TimedValueCache {
    ttl: Duration,
    capacity: usize,
    entries: RwLock<HashMap<String, CachedValue>>,
}

#[derive(Debug, Clone)]
struct CachedValue {
    inserted_at: Instant,
    value: Value,
}

impl TimedValueCache {
    pub(crate) fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            ttl,
            capacity,
            entries: RwLock::new(HashMap::new()),
        }
    }

    pub(crate) async fn get(&self, key: &str) -> Option<Value> {
        let entries = self.entries.read().await;
        entries.get(key).and_then(|entry| {
            if entry.inserted_at.elapsed() <= self.ttl {
                Some(entry.value.clone())
            } else {
                None
            }
        })
    }

    pub(crate) async fn insert(&self, key: String, value: Value) {
        if self.capacity == 0 {
            return;
        }
        let mut entries = self.entries.write().await;
        if entries.len() >= self.capacity {
            if let Some(first_key) = entries.keys().next().cloned() {
                entries.remove(&first_key);
            }
        }
        entries.insert(
            key,
            CachedValue {
                inserted_at: Instant::now(),
                value,
            },
        );
    }
}
