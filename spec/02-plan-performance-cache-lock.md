# 计划：性能（缓存 / 锁 / 分配）

目标：在不牺牲可读性与正确性的前提下，降低热点路径的分配与锁竞争；优化必须以基准数据为依据。

## 原则

- **先量化再优化**：先建立基准（bench），再动缓存/锁结构。
- **热路径优先**：SQL 构建、statement cache、SQL parse cache、stream instrumentation。
- **尽量不加依赖**：除非基准数据证明“自己写不值得/风险更大”，再考虑引入成熟 LRU crate。

## 建议关注点（可能的热点）

- statement cache（`PgClient` 内部 `Mutex<HashMap + VecDeque>`）
- SQL parse cache（`pgorm-check` 的 `SqlParseCache`）
- `Sql::to_sql()` / 条件拼接（频繁构建字符串）
- 一些全局缓存（`OnceLock<Mutex<...>>`）在高并发下的锁竞争

## 工作步骤

### Phase 0：建立基准

- [ ] 添加 `benches/`（推荐 criterion；也可先用简单 `Instant` micro-bench）
- [ ] 至少覆盖 3 类场景：
  - SQL builder：拼接 + bind（不同长度与参数量）
  - statement cache：hit/miss/evict
  - SQL parse cache：hit/miss/evict（不同 SQL 分布）
- [ ] 记录 baseline 数字（在 PR 描述里附上）

### Phase 1：锁与缓存结构优化（数据驱动）

- [ ] 避免在锁内做重活（解析/格式化等）
- [ ] 读多写少的缓存：评估 `RwLock` 或分片（sharding）是否有收益
- [ ] LRU 复杂度：如果 `VecDeque::position()` 确认是热点，再换结构（或引入 crate）

### Phase 2：减少分配与 clone

- [ ] 对热点 String 做 capacity 预估与复用
- [ ] 减少 `to_string()` 的 key 构造次数（尤其是 SQL cache key）
- [ ] 对 `Arc`/`String` clone 做采样（bench + 代码审视）

### Phase 3：可配置与可观测

- [ ] 把关键缓存容量/启用开关暴露在 config（并给默认值理由）
- [ ] 若已有监控：增加命中率/淘汰数统计（不强求，按需求）

## 验收标准（Definition of Done）

- [ ] 有可复现的 bench（本地一条命令可跑）
- [ ] 至少一个明确热点得到改善（或证明现状已足够好）
- [ ] 改动不引入行为差异（测试覆盖）

