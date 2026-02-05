---
pageType: home

hero:
  name: pgorm
  text: Rust PostgreSQL ORM
  tagline: 模型定义优先、AI 友好的 PostgreSQL ORM 库
  actions:
    - theme: brand
      text: 快速开始
      link: /zh/guide/
    - theme: alt
      text: GitHub
      link: https://github.com/fankaiLiu/pgorm
  image:
    src: /rspress-icon.png
    alt: pgorm Logo
features:
  - title: 模型优先设计
    details: 使用派生宏定义模型，pgorm 自动生成插入、更新、查询和关系加载方法。
    icon: 📝
  - title: 类型安全查询
    details: FromRow、Model、PgEnum、PgComposite 派生宏提供编译期安全保障，零运行时开销。
    icon: 🔒
  - title: 关系与预加载
    details: 显式批量预加载 has_many、belongs_to、has_one、many_to_many 关系，杜绝 N+1 查询。
    icon: ⚡
  - title: PostgreSQL 类型
    details: 原生支持 JSONB、ENUM、复合类型、范围类型及所有标准 PG 类型。
    icon: 📦
  - title: 高级查询
    details: CTE (WITH) 查询、游标分页、流式查询、批量操作和乐观锁。
    icon: 🔄
  - title: 安全与监控
    details: 运行时 SQL 检查、安全策略、查询监控、Hook 和 LRU 预处理语句缓存。
    icon: 🛡️
---
