//! LRU storage for idle GPU resources. Active/in-flight handles own their leases.
use std::collections::VecDeque;

pub(super) struct ResourcePool<K, V> {
    entries: VecDeque<(K, V, u64)>,
    bytes: u64,
    limit: u64,
    capacity: usize,
}
impl<K: PartialEq, V> ResourcePool<K, V> {
    pub fn new(limit: u64, capacity: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
            limit,
            capacity,
        }
    }
    pub fn take(&mut self, key: &K) -> Option<V> {
        let index = self.entries.iter().rposition(|entry| entry.0 == *key)?;
        let (_, value, bytes) = self.entries.remove(index).unwrap();
        self.bytes -= bytes;
        Some(value)
    }
    pub fn put(&mut self, key: K, value: V, bytes: u64) {
        if bytes > self.limit || self.capacity == 0 {
            return;
        }
        while self.bytes > self.limit - bytes || self.entries.len() >= self.capacity {
            self.bytes -= self.entries.pop_front().unwrap().2;
        }
        self.bytes += bytes;
        self.entries.push_back((key, value, bytes));
    }
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[test]
    fn reuse_updates_accounting_and_eviction_preserves_live_handles() {
        let mut pool = ResourcePool::new(10, 2);
        let lease = Arc::new(vec![1, 2, 3]);
        pool.put("a", lease.clone(), 6);
        pool.put("b", Arc::new(vec![4]), 4);
        assert_eq!(pool.bytes(), 10);
        assert_eq!(*pool.take(&"b").unwrap(), [4]);
        assert_eq!(pool.bytes(), 6);
        pool.put("c", Arc::new(vec![5]), 7);
        assert!(pool.take(&"a").is_none());
        assert_eq!(lease.as_slice(), [1, 2, 3]);
        pool.put("large", Arc::new(vec![6]), 11);
        assert!(pool.take(&"large").is_none());
        assert_eq!(pool.bytes(), 7);
    }
    #[test]
    fn animated_sizes_and_many_tiny_buffers_cannot_grow_an_idle_pool() {
        let mut pool = ResourcePool::new(128, 16);
        for size in 1..4096u64 {
            pool.put(size, size, size);
            assert!(pool.bytes() <= 128);
            assert!(pool.entries.len() <= 16);
        }
        for key in 0..4096u64 {
            pool.put(key, key, 0);
            assert!(pool.entries.len() <= 16);
        }
    }
}
