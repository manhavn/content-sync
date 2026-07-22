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

### Build release đa nền tảng

```bash
# fmt → build linux (gnu/musl × amd64/aarch64), windows, macOS (nếu có tool)
./scripts/build-release-multi.sh

# xem builder cho từng target
./scripts/build-release-multi.sh --list

# chỉ một số target
./scripts/build-release-multi.sh --only x86_64-unknown-linux-musl,aarch64-unknown-linux-musl
```

Artifact trong `dist/`: `content-sync-v<ver>-<target>[.exe]` (+ `SHA256SUMS.txt`).

**Ưu tiên builder:** host `cargo` → `cargo-zigbuild`+zig → `cross`+podman/docker.

- **Linux musl** = static (không phụ thuộc distro).
- **macOS / một số Windows** cần `cargo-zigbuild` + `zig` (hoặc máy native).
- Smoke-test `--version`/`--help` khi chạy được trên host; không chạy được thì chỉ build.

Release host-only: `./scripts/build-release.sh`.

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
| `content-sync sync` | Đồng bộ 1 lần rồi thoát (Dashboard “Sync now”) |
| `content-sync status` | Xem cấu hình (kèm trạng thái background) |
| `content-sync logs` | Sync logs gần đây (`--limit`, `--level`) |
| `content-sync settings show` / `settings set …` | Xem / cập nhật settings |
| `content-sync export` | Export cấu hình hệ thống ra `export.content.sync.YYYY-MM-DD.HH-MM-SS.json` |
| `content-sync export -o backup.json` | Export ra đường dẫn tùy chọn |
| `content-sync import <file>` | Import cấu hình (có hỏi xác nhận; dùng `-y` để bỏ qua) |
| `content-sync token create/show/list/set/delete` | Quản lý auth tokens |
| `content-sync connection add/list/show/set/toggle/clone/test/delete` | Quản lý DB connections (name hoặc id) |
| `content-sync file list/show/write/delete` | Quản lý file watch (connection theo name hoặc id) |

**Web UI ↔ CLI (đủ bộ)**

| Web | CLI |
|-----|-----|
| Dashboard / Sync now / logs | `sync`, `logs`, `status` |
| Files | `file list\|show\|write\|delete` |
| Connections (add, on/off, test, clone, edit, delete) | `connection add\|toggle\|test\|clone\|set\|delete\|show\|list` |
| Auth Tokens | `token create\|list\|set\|delete\|show` |
| Settings + export/import | `settings show\|set`, `export`, `import` |

Export/import chỉ gồm **settings, connections (kèm secret), auth tokens**.  
**Không** gồm: sync logs, file cache, hay nội dung file trong watch dir.

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
5. **Settings** — poll, backoff, log retention, **export/import cấu hình**  

Language: EN (default) / VI toggle (localStorage).

## API

- `POST /api/login` · `GET /api/me`
- `GET  /api/status` · `POST /api/sync`
- `GET/POST /api/files` · `PUT/DELETE /api/files/:name` (`{ "file_name", "content" }`)
- `GET/POST /api/connections` · `PUT/DELETE /api/connections/:id`
- `GET/PUT /api/settings`
- `GET  /api/config/export` · `POST /api/config/import` (chỉ config; không logs/files)
