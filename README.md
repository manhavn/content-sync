# Content Sync

**English** · [Tiếng Việt](./README.VI.md)

Rust CLI to bidirectionally sync **raw files** (any format) with one or more databases (Bunny / SQL / MongoDB), with a built-in **Web UI**.

## Features

- **Multi-connection** — each connection = one DB + one table/collection + one local directory (`watch_dir`)
- **Multi-driver** — `sql_api`, `libsql`, `sqlite`, `postgres`, `mysql`, `mariadb`, `mongodb`
- **Multi-file** — each watch dir can hold many raw files (any format)
- **Pull/push** — independent per connection (many DBs / tables / dirs at once)
- **Web UI** — files (pick a connection), connections, auth tokens, settings
- **Local config** — `~/.content-sync/config.sqlite`

## Install

```bash
cargo build --release
# binary: ./target/release/content-sync
```

## Quick start

```bash
content-sync init
# or
content-sync init --watch-dir ~/my-files

# Bunny SQL API
content-sync connection add \
  --name prod \
  --driver sql_api \
  --url 'https://YOUR-DB-ID.lite.bunnydb.net/v2/pipeline' \
  --access-token 'YOUR_ACCESS_TOKEN' \
  --table content_syncs \
  --watch-dir ~/.content-sync/files/prod

# libSQL SDK
content-sync connection add \
  --name staging \
  --driver libsql \
  --url 'https://YOUR-DB-ID.lite.bunnydb.net' \
  --access-token 'YOUR_ACCESS_TOKEN' \
  --table content_syncs \
  --watch-dir ~/sync/staging

# SQLite file
content-sync connection add \
  --name local-sqlite \
  --driver sqlite \
  --url '/var/data/sync.db' \
  --table content_syncs \
  --watch-dir ~/sync/sqlite

# PostgreSQL (password may be in the DSN or --access-token)
content-sync connection add \
  --name pg \
  --driver postgres \
  --url 'postgresql://user@127.0.0.1:5432/mydb' \
  --access-token 'secret' \
  --table content_syncs \
  --watch-dir ~/sync/pg

# MySQL / MariaDB
content-sync connection add \
  --name mysql \
  --driver mysql \
  --url 'mysql://user@127.0.0.1:3306/mydb' \
  --access-token 'secret' \
  --table content_syncs \
  --watch-dir ~/sync/mysql

# MongoDB (table = collection; DB name from URL path, default content_sync)
content-sync connection add \
  --name mongo \
  --driver mongodb \
  --url 'mongodb://127.0.0.1:27017/content_sync' \
  --table content_syncs \
  --watch-dir ~/sync/mongo

content-sync connection test <id>
content-sync serve
# → http://127.0.0.1:8787
```

## CLI

| Command | Description |
|---------|-------------|
| `content-sync init` | Create `~/.content-sync`, settings, admin auth token |
| `content-sync serve` | Web UI + file watcher + poll sync (foreground) |
| `content-sync serve --bind 0.0.0.0:8787` | Custom bind address |
| `content-sync serve --no-sync` | Web/API only, no watcher |
| `content-sync serve --no-log` | Silence runtime logs (tracing + serve banner) |
| `content-sync background` | Same as `serve`, but runs as a background daemon |
| `content-sync background --bind 0.0.0.0:8787` | Background with custom bind |
| `content-sync background --no-log` | Background with silent core logs (no `content-sync.log`) |
| `content-sync quit` | Stop the background daemon |
| `content-sync sync` | One-shot sync, then exit |
| `content-sync status` | Show configuration (includes background PID status) |
| `content-sync export` | Export system config to `export.content.sync.YYYY-MM-DD.HH-MM-SS.json` |
| `content-sync export -o backup.json` | Export to a custom path |
| `content-sync import <file>` | Import config (prompts; use `-y` to skip) |
| `content-sync token create --name laptop` | Create Web UI login token |
| `content-sync token show admin` | Print raw admin token |
| `content-sync token list` / `delete` / `set` | Manage auth tokens |
| `content-sync connection add/list/set/delete/test` | Manage DB connections |

Config export/import covers **settings, connections (with secrets), auth tokens** only.  
**Not** included: sync logs, file cache, or file contents under watch dirs.

### Drivers

| Driver | URL / DSN | Secret (`--access-token`) | Remote object |
|--------|-----------|---------------------------|---------------|
| `sql_api` (default) | `…/v2/pipeline` — [sql-api.md](./sql-api.md) | Required (Bunny token) | SQL table |
| `libsql` | `https://…` or `libsql://…` — [sdk-rust.md](./sdk-rust.md) | Required | SQL table |
| `sqlite` | path or `sqlite:/path/to.db` | Not needed | SQL table |
| `postgres` | `postgresql://user@host/db` | Password (if not already in DSN) | SQL table |
| `mysql` | `mysql://user@host/db` | Password (if not already in DSN) | SQL table |
| `mariadb` | `mysql://user@host/db` | Password (if not already in DSN) | SQL table |
| `mongodb` | `mongodb://host/db` or `mongodb+srv://…` | Password (if not already in URI) | Collection |

Remote schema (every SQL driver / Mongo document): `id`, `file_name` (unique), `content`, `content_hash`, `updated_at`.

## Config

| Path | Contents |
|------|----------|
| `~/.content-sync/config.sqlite` | Auth tokens, connections, settings, cache, sessions |
| `~/.content-sync/files/<name>/` | Default watch dir per connection (one dir each) |
| `~/.content-sync/content-sync.pid` | PID of the background daemon (`background` / `quit`) |
| `~/.content-sync/content-sync.log` | stdout/stderr of the background daemon |

## Web UI

1. **Dashboard** — sync status, logs, Sync now  
2. **Files** — CRUD raw text files → write local + push remote  
3. **Connections** — CRUD URL/token; driver; enable/disable; Test/migrate  
4. **Auth Tokens** — UI login tokens  
5. **Settings** — poll, backoff, log retention, **export/import config**  

Language: EN (default) / VI toggle (localStorage).

## API

- `POST /api/login` · `GET /api/me`
- `GET  /api/status` · `POST /api/sync`
- `GET/POST /api/files` · `PUT/DELETE /api/files/:name` (`{ "file_name", "content" }`)
- `GET/POST /api/connections` · `PUT/DELETE /api/connections/:id`
- `GET/PUT /api/settings`
- `GET  /api/config/export` · `POST /api/config/import` (config only; no logs/files)
