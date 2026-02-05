//! Benchmark the LRU cache pattern used by StatementCache.
//!
//! We benchmark the generation-counter approach (O(1) touch, O(n) evict)
//! which matches the actual StatementCache implementation.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use std::collections::HashMap;

/// Generation-counter LRU cache (matches actual StatementCache implementation).
struct LruCache<V> {
    capacity: usize,
    map: HashMap<String, (V, u64)>,
    generation: u64,
}

impl<V: Clone> LruCache<V> {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            map: HashMap::with_capacity(capacity),
            generation: 0,
        }
    }

    fn get(&mut self, key: &str) -> Option<V> {
        let entry = self.map.get_mut(key)?;
        self.generation += 1;
        entry.1 = self.generation;
        Some(entry.0.clone())
    }

    fn insert(&mut self, key: String, value: V) {
        if let Some(entry) = self.map.get_mut(&key) {
            self.generation += 1;
            entry.1 = self.generation;
            return;
        }
        self.generation += 1;
        let access = self.generation;
        self.map.insert(key, (value, access));
        self.evict();
    }

    fn evict(&mut self) {
        while self.map.len() > self.capacity {
            let oldest_key = self
                .map
                .iter()
                .min_by_key(|(_, (_, last_access))| *last_access)
                .map(|(k, _)| k.clone());

            if let Some(key) = oldest_key {
                self.map.remove(&key);
            } else {
                break;
            }
        }
    }
}

fn make_key(i: usize) -> String {
    format!("SELECT * FROM table_{i} WHERE id = $1 AND status = $2")
}

fn bench_cache_hit(c: &mut Criterion) {
    let mut group = c.benchmark_group("lru_cache/hit");

    for capacity in [64, 256, 1024] {
        let mut cache = LruCache::new(capacity);
        for i in 0..capacity {
            cache.insert(make_key(i), i as u64);
        }

        let hit_key = make_key(capacity / 2);
        group.bench_with_input(BenchmarkId::from_parameter(capacity), &hit_key, |b, key| {
            b.iter(|| black_box(cache.get(key)));
        });
    }

    group.finish();
}

fn bench_cache_miss_and_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("lru_cache/miss_insert");

    for capacity in [64, 256, 1024] {
        group.bench_with_input(
            BenchmarkId::from_parameter(capacity),
            &capacity,
            |b, &cap| {
                let mut cache = LruCache::new(cap);
                for i in 0..cap {
                    cache.insert(make_key(i), i as u64);
                }
                let mut counter = cap;
                b.iter(|| {
                    counter += 1;
                    let key = make_key(counter);
                    cache.insert(key, counter as u64);
                });
            },
        );
    }

    group.finish();
}

fn bench_cache_mixed_workload(c: &mut Criterion) {
    let mut group = c.benchmark_group("lru_cache/mixed");

    for capacity in [64, 256, 1024] {
        group.bench_with_input(
            BenchmarkId::from_parameter(capacity),
            &capacity,
            |b, &cap| {
                let mut cache = LruCache::new(cap);
                let prefill = cap * 4 / 5;
                for i in 0..prefill {
                    cache.insert(make_key(i), i as u64);
                }
                let mut counter = 0usize;
                b.iter(|| {
                    counter += 1;
                    if counter % 5 == 0 {
                        let key = make_key(cap + counter);
                        cache.insert(key, counter as u64);
                    } else {
                        let key = make_key(counter % prefill);
                        black_box(cache.get(&key));
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_cache_hit,
    bench_cache_miss_and_insert,
    bench_cache_mixed_workload
);
criterion_main!(benches);
