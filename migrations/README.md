# Migrations

This folder contains migrations; they are automatically ran on start-up.

## Format
```
{unix timestamp}_{description}.sql
```

## Contributors
To create a new migration run `./new.sh [description]`. The description must not have any spaces, use `_` in place of any space.