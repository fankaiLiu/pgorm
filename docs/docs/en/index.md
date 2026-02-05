---
pageType: home

hero:
  name: pgorm
  text: PostgreSQL ORM for Rust
  tagline: A model-definition-first, AI-friendly ORM library for PostgreSQL
  actions:
    - theme: brand
      text: Quick Start
      link: /en/guide/
    - theme: alt
      text: GitHub
      link: https://github.com/fankaiLiu/pgorm
  image:
    src: /rspress-icon.png
    alt: pgorm Logo
features:
  - title: Model-First Design
    details: Define models with derive macros. pgorm generates insert, update, query, and relation helpers automatically.
    icon: 📝
  - title: Type-Safe Queries
    details: FromRow, Model, PgEnum, PgComposite derive macros provide compile-time safety with zero runtime overhead.
    icon: 🔒
  - title: Relations & Eager Loading
    details: Explicit batch preloading for has_many, belongs_to, has_one, and many_to_many relations. No N+1 queries.
    icon: ⚡
  - title: PostgreSQL Types
    details: First-class support for JSONB, ENUM, Composite Types, Range types, and all standard PG types.
    icon: 📦
  - title: Advanced Queries
    details: CTE (WITH) queries, keyset pagination, streaming, bulk operations, and optimistic locking.
    icon: 🔄
  - title: Safety & Monitoring
    details: Runtime SQL checking, safety policies, query monitoring, hooks, and LRU statement cache.
    icon: 🛡️
---
