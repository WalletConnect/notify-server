# Migrations

This folder contains migrations for Notify Server and they are automatically run on start-up.

If you make a change, please also update [ERD.md](./ERD.md).

## New Migration

```bash
cargo install sqlx-cli
```

```bash
sqlx migrate add <name>
```
