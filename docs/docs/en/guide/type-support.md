# Type Support (Extra Types)

pgorm is built on `tokio-postgres`, so “type support” has two layers:

1) **Binding & decoding** (works for `query()` / `sql()`):
   - bind parameters via `ToSql`
   - decode columns via `FromSql`

2) **UNNEST bulk writes** (used by `InsertModel::insert_many*` / `upsert_many*`):
   - requires `pgorm::PgType` so pgorm can generate the right `$n::<array_type>` casts

Some types need extra, feature-gated support. pgorm provides a set of opt-in features that:

- enable `pgorm::PgType` impls, and
- enable the corresponding `tokio-postgres` `with-*` features (when applicable)

## Enable all common extras

```toml
[dependencies]
pgorm = { version = "0.1.4", features = ["extra_types"] }
```

## Per-type features

| Type | Postgres type | pgorm feature | Notes |
| --- | --- | --- | --- |
| `rust_decimal::Decimal` | `numeric` | `rust_decimal` | You must enable `rust_decimal`’s Postgres support in your app (e.g. `rust_decimal = { version = "1", features = ["tokio-pg"] }`). |
| `time::Date/Time/PrimitiveDateTime/OffsetDateTime` | `date/time/timestamp/timestamptz` | `time` | Also enables `tokio-postgres/with-time-0_3`. |
| `cidr::IpCidr` / `cidr::IpInet` | `cidr` / `inet` | `cidr` | Also enables `tokio-postgres/with-cidr-0_3`. |
| `geo_types::Point<f64>` / `Rect<f64>` / `LineString<f64>` | `point` / `box` / `path` | `geo_types` | Also enables `tokio-postgres/with-geo-types-0_7`. |
| `eui48::MacAddress` | `macaddr` | `eui48` | Also enables `tokio-postgres/with-eui48-1`. |
| `bit_vec::BitVec` | `bit` / `varbit` | `bit_vec` | Also enables `tokio-postgres/with-bit-vec-0_8`. |

> Note: even if you enable a pgorm feature, you still need to add the corresponding type crate
> (`time`, `cidr`, `geo-types`, etc.) as a **direct** dependency if you want to reference those types in your code.

