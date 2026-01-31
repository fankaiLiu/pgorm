# 类型支持（额外类型）

pgorm 基于 `tokio-postgres`，所以“类型支持”通常分两层：

1) **bind / decode**（适用于 `query()` / `sql()`）：
   - 参数绑定：`ToSql`
   - 列解码：`FromSql`

2) **UNNEST 批量写入**（`InsertModel::insert_many*` / `upsert_many*` 会用到）：
   - 需要 `pgorm::PgType`，用于生成正确的 `$n::<array_type>` cast

部分类型需要额外（feature-gated）的支持。pgorm 提供了一组可选 feature，用于：

- 开启对应的 `pgorm::PgType` 实现
- 同时开启 `tokio-postgres` 的 `with-*` feature（如果该类型由 tokio-postgres 管理）

## 一次开启常见额外类型

```toml
[dependencies]
pgorm = { version = "0.1.4", features = ["extra_types"] }
```

## 按类型开启

| 类型 | PG 类型 | pgorm feature | 备注 |
| --- | --- | --- | --- |
| `rust_decimal::Decimal` | `numeric` | `rust_decimal` | 你的业务工程里还需要开启 `rust_decimal` 的 PG 支持（例如 `rust_decimal = { version = "1", features = ["tokio-pg"] }`），否则只能过 `PgType`，bind/get 仍然不行。 |
| `time::Date/Time/PrimitiveDateTime/OffsetDateTime` | `date/time/timestamp/timestamptz` | `time` | 同时开启 `tokio-postgres/with-time-0_3`。 |
| `cidr::IpCidr` / `cidr::IpInet` | `cidr` / `inet` | `cidr` | 同时开启 `tokio-postgres/with-cidr-0_3`。 |
| `geo_types::Point<f64>` / `Rect<f64>` / `LineString<f64>` | `point` / `box` / `path` | `geo_types` | 同时开启 `tokio-postgres/with-geo-types-0_7`。 |
| `eui48::MacAddress` | `macaddr` | `eui48` | 同时开启 `tokio-postgres/with-eui48-1`。 |
| `bit_vec::BitVec` | `bit` / `varbit` | `bit_vec` | 同时开启 `tokio-postgres/with-bit-vec-0_8`。 |

> 注意：即使你开启了 pgorm 的 feature，如果你想在业务代码里直接使用这些类型（`time::...`、`cidr::...` 等），仍然需要把对应 crate 写进你自己的 `Cargo.toml`（作为直接依赖）。

