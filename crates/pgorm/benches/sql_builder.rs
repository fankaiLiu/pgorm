use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use pgorm::Sql;

/// Build an Sql with `n` columns and `n` bind parameters:
/// SELECT col0, col1, ... FROM t WHERE col0 = $1 AND col1 = $2 ...
fn build_select_sql(n: usize) -> Sql {
    let mut sql = Sql::new("SELECT ");
    for i in 0..n {
        if i > 0 {
            sql.push(", ");
        }
        sql.push(&format!("col{i}"));
    }
    sql.push(" FROM t WHERE ");
    for i in 0..n {
        if i > 0 {
            sql.push(" AND ");
        }
        sql.push(&format!("col{i} = "));
        sql.push_bind(i as i64);
    }
    sql
}

fn bench_to_sql(c: &mut Criterion) {
    let mut group = c.benchmark_group("sql_builder/to_sql");

    for n in [1, 5, 10, 50, 100] {
        let sql = build_select_sql(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &sql, |b, sql| {
            b.iter(|| black_box(sql.to_sql()));
        });
    }

    group.finish();
}

fn bench_build_and_render(c: &mut Criterion) {
    let mut group = c.benchmark_group("sql_builder/build_and_render");

    for n in [1, 5, 10, 50, 100] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| {
                let sql = build_select_sql(n);
                black_box(sql.to_sql());
            });
        });
    }

    group.finish();
}

fn bench_push_bind_list(c: &mut Criterion) {
    let mut group = c.benchmark_group("sql_builder/push_bind_list");

    for n in [5, 20, 100, 500] {
        let values: Vec<i64> = (0..n).collect();
        group.bench_with_input(BenchmarkId::from_parameter(n), &values, |b, values| {
            b.iter(|| {
                let mut sql = Sql::new("SELECT * FROM t WHERE id IN (");
                sql.push_bind_list(values.iter().copied());
                sql.push(")");
                black_box(sql.to_sql());
            });
        });
    }

    group.finish();
}

fn bench_condition_append(c: &mut Criterion) {
    use pgorm::Condition;

    let mut group = c.benchmark_group("sql_builder/condition_append");

    for n in [1, 5, 10, 50] {
        let conditions: Vec<Condition> = (0..n)
            .map(|i| Condition::eq(format!("col{i}"), i as i64).unwrap())
            .collect();

        group.bench_with_input(BenchmarkId::from_parameter(n), &conditions, |b, conds| {
            b.iter(|| {
                let mut sql = Sql::new("SELECT * FROM t WHERE ");
                sql.push_conditions_and(conds);
                black_box(sql.to_sql());
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_to_sql,
    bench_build_and_render,
    bench_push_bind_list,
    bench_condition_append
);
criterion_main!(benches);
