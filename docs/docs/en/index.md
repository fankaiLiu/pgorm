---
pageType: home

hero:
  name: pgorm
  text: PostgreSQL ORM for Rust
  tagline: A SQL-first, type-safe ORM library for PostgreSQL
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
  - title: Under Active Development
    details: This project is rapidly evolving. APIs may change. Not recommended for production use yet.
    icon: 🚧
  - title: SQL-First Approach
    details: Keep SQL explicit with query() for full SQL strings and sql() for dynamic composition.
    icon: 📝
  - title: Type-Safe Queries
    details: Derive macros (FromRow, Model) provide compile-time safety with zero runtime overhead.
    icon: 🔒
  - title: Eager Loading
    details: Explicit batch preloading for has_many and belongs_to relations - no N+1 queries.
    icon: ⚡
  - title: JSONB Support
    details: First-class support for PostgreSQL JSONB with serde integration.
    icon: 📦
  - title: Connection Pooling
    details: Built-in deadpool-postgres integration with TLS support.
    icon: 🔌
  - title: Runtime SQL Checking
    details: Optional guardrails for AI-generated SQL with CheckedClient and PgClient.
    icon: 🛡️
---
