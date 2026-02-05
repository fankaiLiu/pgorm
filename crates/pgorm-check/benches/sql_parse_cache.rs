use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use pgorm_check::SqlParseCache;

fn make_sql(i: usize) -> String {
    format!(
        "SELECT id, name, email, status FROM users_{i} WHERE id = $1 AND status = $2 ORDER BY name LIMIT 100"
    )
}

fn bench_parse_cache_hit(c: &mut Criterion) {
    let mut group = c.benchmark_group("sql_parse_cache/hit");

    for capacity in [64, 256, 1024] {
        let cache = SqlParseCache::new(capacity);
        // Pre-fill
        for i in 0..capacity.min(200) {
            cache.analyze(&make_sql(i));
        }

        let hit_sql = make_sql(0);
        group.bench_with_input(BenchmarkId::from_parameter(capacity), &hit_sql, |b, sql| {
            b.iter(|| black_box(cache.analyze(sql)));
        });
    }

    group.finish();
}

fn bench_parse_cache_miss(c: &mut Criterion) {
    let mut group = c.benchmark_group("sql_parse_cache/miss");

    // Miss = parse + insert. This benchmarks the heavy pg_query::parse path.
    for capacity in [64, 256] {
        let cache = SqlParseCache::new(capacity);
        // Pre-fill to capacity so each miss triggers eviction
        for i in 0..capacity {
            cache.analyze(&make_sql(i));
        }

        let mut counter = capacity;
        group.bench_with_input(BenchmarkId::from_parameter(capacity), &capacity, |b, _| {
            b.iter(|| {
                counter += 1;
                let sql = make_sql(counter);
                black_box(cache.analyze(&sql));
            });
        });
    }

    group.finish();
}

fn bench_parse_cache_mixed(c: &mut Criterion) {
    let mut group = c.benchmark_group("sql_parse_cache/mixed");

    for capacity in [64, 256] {
        let cache = SqlParseCache::new(capacity);
        let prefill = capacity * 4 / 5;
        for i in 0..prefill {
            cache.analyze(&make_sql(i));
        }

        let mut counter = 0usize;
        group.bench_with_input(BenchmarkId::from_parameter(capacity), &capacity, |b, _| {
            b.iter(|| {
                counter += 1;
                if counter % 5 == 0 {
                    // 20% miss
                    let sql = make_sql(capacity + counter);
                    black_box(cache.analyze(&sql));
                } else {
                    // 80% hit
                    let sql = make_sql(counter % prefill);
                    black_box(cache.analyze(&sql));
                }
            });
        });
    }

    group.finish();
}

fn bench_analyze_sql_no_cache(c: &mut Criterion) {
    let mut group = c.benchmark_group("sql_parse_cache/raw_parse");

    // Benchmark raw parsing without cache (capacity=0)
    let cache = SqlParseCache::new(0);

    for complexity in ["simple", "medium", "complex"] {
        let sql = match complexity {
            "simple" => "SELECT 1".to_string(),
            "medium" => {
                "SELECT id, name, email FROM users WHERE status = $1 AND created_at > $2 ORDER BY name LIMIT 100".to_string()
            }
            "complex" => {
                "SELECT u.id, u.name, o.total, p.name AS product FROM users u JOIN orders o ON o.user_id = u.id JOIN order_items oi ON oi.order_id = o.id JOIN products p ON p.id = oi.product_id WHERE u.status = $1 AND o.created_at > $2 GROUP BY u.id, u.name, o.total, p.name HAVING o.total > $3 ORDER BY o.total DESC LIMIT 50".to_string()
            }
            _ => unreachable!(),
        };

        group.bench_with_input(BenchmarkId::from_parameter(complexity), &sql, |b, sql| {
            b.iter(|| black_box(cache.analyze(sql)));
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_parse_cache_hit,
    bench_parse_cache_miss,
    bench_parse_cache_mixed,
    bench_analyze_sql_no_cache
);
criterion_main!(benches);
