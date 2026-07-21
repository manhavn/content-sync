# Content Sync

[English](./README.md) · **Tiếng Việt**

CLI Rust đồng bộ **file raw** (mọi định dạng) với một hoặc nhiều database (Bunny / SQL / MongoDB), kèm **Web UI**.

## Tính năng

- **Multi-connection** — mỗi connection = 1 DB + 1 bảng/collection + 1 thư mục local (`watch_dir`)
- **Multi-driver** — `sql_api`, `libsql`, `sqlite`, `postgres`, `mysql`, `mariadb`, `mongodb`
- **Multi-file** — mỗi dir chứa nhiều file raw (mọi định dạng)
- **Pull/push** — theo từng connection độc lập (nhiều DB / nhiều bảng / nhiều dir cùng lúc)
- **Web UI** — files (chọn connection), connections, auth tokens, settings
- **Config local** — `~/.content-sync/config.sqlite`

## Cài đặt

```bash
cargo build --release
# binary: ./target/release/content-sync
```

## Quick start

```bash
content-sync init
# hoặc
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

# PostgreSQL (password có thể nằm trong DSN hoặc --access-token)
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

# MongoDB (table = collection; DB name lấy từ path URL, mặc định content_sync)
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

| Lệnh | Mô tả |
|------|--------|
| `content-sync init` | Tạo `~/.content-sync`, settings, auth token admin |
| `content-sync serve` | Web UI + file watcher + poll sync (foreground) |
| `content-sync serve --bind 0.0.0.0:8787` | Bind tùy chỉnh |
| `content-sync serve --no-sync` | Chỉ Web/API, không watcher |
| `content-sync serve --no-log` | Tắt log runtime (tracing + banner serve) |
| `content-sync background` | Giống `serve`, chạy nền (daemon) |
| `content-sync background --bind 0.0.0.0:8787` | Chạy nền với bind tùy chỉnh |
| `content-sync background --no-log` | Chạy nền, tắt log core (không ghi `content-sync.log`) |
| `content-sync quit` | Dừng process background |
| `content-sync sync` | Đồng bộ 1 lần rồi thoát |
| `content-sync status` | Xem cấu hình (kèm trạng thái background) |
| `content-sync token create --name laptop` | Tạo token login Web UI |
| `content-sync token show admin` | In raw token |
| `content-sync token list` / `delete` / `set` | Quản lý auth tokens |
| `content-sync connection add/list/set/delete/test` | Quản lý DB connections |

### Drivers

| Driver | URL / DSN | Secret (`--access-token`) | Remote object |
|--------|-----------|---------------------------|---------------|
| `sql_api` (default) | `…/v2/pipeline` — [sql-api.md](./sql-api.md) | Bắt buộc (Bunny token) | SQL table |
| `libsql` | `https://…` hoặc `libsql://…` — [sdk-rust.md](./sdk-rust.md) | Bắt buộc | SQL table |
| `sqlite` | path hoặc `sqlite:/path/to.db` | Không cần | SQL table |
| `postgres` | `postgresql://user@host/db` | Password (nếu chưa có trong DSN) | SQL table |
| `mysql` | `mysql://user@host/db` | Password (nếu chưa có trong DSN) | SQL table |
| `mariadb` | `mysql://user@host/db` | Password (nếu chưa có trong DSN) | SQL table |
| `mongodb` | `mongodb://host/db` hoặc `mongodb+srv://…` | Password (nếu chưa có trong URI) | Collection |

Remote schema (mọi driver SQL / Mongo document): `id`, `file_name` (unique), `content`, `content_hash`, `updated_at`.

## Config

| Path | Nội dung |
|------|----------|
| `~/.content-sync/config.sqlite` | Auth tokens, connections, settings, cache, sessions |
| `~/.content-sync/files/<name>/` | Watch dir mặc định theo connection (mỗi connection một dir) |
| `~/.content-sync/content-sync.pid` | PID daemon background (`background` / `quit`) |
| `~/.content-sync/content-sync.log` | stdout/stderr của daemon background |

## Web UI

1. **Dashboard** — trạng thái sync, log, Sync now  
2. **Files** — CRUD file raw text → ghi local + push remote  
3. **Connections** — CRUD URL/token; driver; Bật/Tắt; Test/migrate  
4. **Auth Tokens** — token đăng nhập UI  
5. **Settings** — watch dir, poll, backoff, log retention  

Language: EN (default) / VI toggle (localStorage).

## API

- `POST /api/login` · `GET /api/me`
- `GET  /api/status` · `POST /api/sync`
- `GET/POST /api/files` · `PUT/DELETE /api/files/:name` (`{ "file_name", "content" }`)
- `GET/POST /api/connections` · `PUT/DELETE /api/connections/:id`
- `GET/PUT /api/settings`
