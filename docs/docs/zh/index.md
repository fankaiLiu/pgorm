---
pageType: home

hero:
  name: pgorm
  text: Rust PostgreSQL ORM
  tagline: 一个 SQL 优先、类型安全的 PostgreSQL ORM 库
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
  - title: 快速开发中
    details: 本项目正在快速迭代开发中，API 可能会有变动，暂不建议用于生产环境。
    icon: 🚧
  - title: SQL 优先
    details: 保持 SQL 显式化，使用 query() 执行完整 SQL，使用 sql() 进行动态组合。
    icon: 📝
  - title: 类型安全
    details: 派生宏（FromRow、Model）提供编译时安全性，零运行时开销。
    icon: 🔒
  - title: 预加载
    details: 显式批量预加载 has_many 和 belongs_to 关系，避免 N+1 查询问题。
    icon: ⚡
  - title: JSONB 支持
    details: 原生支持 PostgreSQL JSONB，集成 serde 序列化。
    icon: 📦
  - title: 连接池
    details: 内置 deadpool-postgres 集成，支持 TLS。
    icon: 🔌
  - title: 运行时 SQL 检查
    details: 可选的 AI 生成 SQL 保护机制，使用 CheckedClient 和 PgClient。
    icon: 🛡️
---
