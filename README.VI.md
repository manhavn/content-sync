# Content Sync

[English](./README.md) · **Tiếng Việt**

CLI Rust đồng bộ **file raw** (mọi định dạng) với một hoặc nhiều database (Bunny / SQL / MongoDB), kèm **Web UI**.

## Tính năng

- **Multi-connection** — mỗi connection = 1 DB + 1 bảng/collection + 1 thư mục local (`watch_dir`)
- **Multi-driver** — `sql_api`, `libsql`, `sqlite`, `postgres`, `mysql`, `mariadb`, `mongodb`
- **Multi-file** — mỗi dir chứa nhiều file raw (mọi định dạng)
- **Pull/push** — theo từng connection độc lập (nhiều DB / nhiều bảng / nhiều dir cùng lúc)
- **Web UI** — files (chọn connection), connections, auth tokens, settings
- **Config local** — `~/.content-sync/config-sqlite`

## Cài đặt

```bash
cargo build --release
# binary: ./target/release/content-sync
```

Release host-only (fmt + check + build):

```bash
./scripts/build-release.sh
# → ./target/release/content-sync
```

## Build release đa nền tảng

Script: [`scripts/build-release-multi.sh`](./scripts/build-release-multi.sh)

Luôn chạy **`cargo fmt`** trước, rồi build release vào **`dist/`**:

```text
dist/content-sync-v<version>-<target>[.exe]
dist/SHA256SUMS.txt
```

**Ưu tiên builder (từng target):** host `cargo` → `cargo-zigbuild` + zig → `cross` + podman/docker.

Đã verify trên host **Linux x86_64** (bảng ma trận bên dưới). Smoke-test runtime (`--version` / `--help`) chỉ chạy khi binary chạy được trên host; macOS / Windows / Linux kiến trúc khác = chỉ build.

### 1. Yêu cầu chung

```bash
# Rust stable + rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup default stable
rustup component add rustfmt

export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
```

### 2. Cài tool (chọn theo target cần build)

#### A. `cross` + Podman/Docker — Linux cross + Windows x86_64

Đã build thành công:

- `x86_64-unknown-linux-musl`
- `aarch64-unknown-linux-gnu`
- `aarch64-unknown-linux-musl`
- `x86_64-pc-windows-gnu`

```bash
# Cài cross
cargo install cross --git https://github.com/cross-rs/cross

# Container engine (podman ưu tiên)
# Debian/Ubuntu ví dụ:
#   sudo apt-get install -y podman
# hoặc Docker Engine

export CROSS_CONTAINER_ENGINE=podman   # hoặc docker
# Lần build đầu sẽ pull image ghcr.io/cross-rs/<target>:main (~1–2 GB / target)
```

#### B. `cargo-zigbuild` + Zig **0.13.x** — macOS + Windows ARM64

Đã build thành công:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `aarch64-pc-windows-gnullvm`

> **Quan trọng:** dùng **Zig 0.13.x**. Zig **0.14+** lỗi link macOS (SDK/sysroot) trong test của chúng tôi.

```bash
# cargo-zigbuild
cargo install cargo-zigbuild

# Zig 0.13.0 (host Linux x86_64)
ZIG_VER=0.13.0
curl -fL "https://ziglang.org/download/${ZIG_VER}/zig-linux-x86_64-${ZIG_VER}.tar.xz" \
  -o /tmp/zig.tar.xz
mkdir -p "$HOME/.local"
tar -xJf /tmp/zig.tar.xz -C "$HOME/.local"
# Tùy tên thư mục sau khi giải nén, ví dụ:
#   ~/.local/zig-linux-x86_64-0.13.0/zig
#   hoặc ~/.local/zig-0.13.0/zig
ln -sfn "$HOME/.local/zig-linux-x86_64-${ZIG_VER}/zig" "$HOME/.local/bin/zig"
# Nếu đã đổi tên:
# ln -sfn "$HOME/.local/zig-0.13.0/zig" "$HOME/.local/bin/zig"

export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
zig version   # kỳ vọng 0.13.0
```

#### C. MacOSX SDK — bắt buộc khi build **apple-darwin** từ Linux

Link macOS cần framework Apple (`CoreFoundation`, `Security`, …):

```bash
mkdir -p "$HOME/.local/macos-sdk"
curl -fL \
  "https://github.com/joseluisq/macosx-sdks/releases/download/11.3/MacOSX11.3.sdk.tar.xz" \
  | tar -xJ -C "$HOME/.local/macos-sdk"

export SDKROOT="$HOME/.local/macos-sdk/MacOSX11.3.sdk"
# build-release-multi.sh tự tìm path này nếu SDKROOT chưa set
ls "$SDKROOT/System/Library/Frameworks" | head
```

### 3. Build multi một lệnh (khuyến nghị)

```bash
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
export CROSS_CONTAINER_ENGINE=podman
# SDKROOT tự detect từ ~/.local/macos-sdk/MacOSX*.sdk nếu có

# Xem builder cho từng target
./scripts/build-release-multi.sh --list

# Full matrix mặc định (skip target thiếu tool)
./scripts/build-release-multi.sh

# Subset
./scripts/build-release-multi.sh --only x86_64-unknown-linux-musl,aarch64-apple-darwin

# Chỉ build, không smoke-test
./scripts/build-release-multi.sh --skip-test
```

### 4. Build thủ công từng target (đã verify)

Giả định: đang ở root repo, PATH có `~/.local/bin` và `~/.cargo/bin`, set `SDKROOT` khi build macOS.

#### Linux x86_64 (host / glibc)

```bash
cargo build --release --target x86_64-unknown-linux-gnu
# hoặc trên host x86_64-unknown-linux-gnu:
# cargo build --release
./target/x86_64-unknown-linux-gnu/release/content-sync --version
```

#### Linux x86_64 musl (static, portable)

```bash
export CROSS_CONTAINER_ENGINE=podman
cross build --release --target x86_64-unknown-linux-musl
file target/x86_64-unknown-linux-musl/release/content-sync
# ELF … static-pie linked
./target/x86_64-unknown-linux-musl/release/content-sync --version
```

#### Linux aarch64 (gnu)

```bash
export CROSS_CONTAINER_ENGINE=podman
rustup target add aarch64-unknown-linux-gnu
cross build --release --target aarch64-unknown-linux-gnu
file target/aarch64-unknown-linux-gnu/release/content-sync
# ELF 64-bit … ARM aarch64 … dynamically linked
# Không chạy được trên host x86_64 nếu không có qemu-aarch64
```

#### Linux aarch64 musl (static)

```bash
export CROSS_CONTAINER_ENGINE=podman
rustup target add aarch64-unknown-linux-musl
cross build --release --target aarch64-unknown-linux-musl
file target/aarch64-unknown-linux-musl/release/content-sync
# ELF 64-bit … ARM aarch64 … statically linked
```

#### Windows x86_64 (gnu)

```bash
export CROSS_CONTAINER_ENGINE=podman
rustup target add x86_64-pc-windows-gnu
cross build --release --target x86_64-pc-windows-gnu
file target/x86_64-pc-windows-gnu/release/content-sync.exe
# PE32+ executable … x86-64
```

#### Windows ARM64 (gnullvm) qua zigbuild

```bash
# cần cargo-zigbuild + zig 0.13.x
rustup target add aarch64-pc-windows-gnullvm
cargo zigbuild --release --target aarch64-pc-windows-gnullvm
file target/aarch64-pc-windows-gnullvm/release/content-sync.exe
# PE32+ executable … ARM64
```

#### macOS Apple Silicon (aarch64) — zigbuild + SDK

```bash
export SDKROOT="$HOME/.local/macos-sdk/MacOSX11.3.sdk"
rustup target add aarch64-apple-darwin
cargo zigbuild --release --target aarch64-apple-darwin
file target/aarch64-apple-darwin/release/content-sync
# Mach-O 64-bit arm64 executable
```

#### macOS Intel (x86_64) — zigbuild + SDK

```bash
export SDKROOT="$HOME/.local/macos-sdk/MacOSX11.3.sdk"
rustup target add x86_64-apple-darwin
cargo zigbuild --release --target x86_64-apple-darwin
file target/x86_64-apple-darwin/release/content-sync
# Mach-O 64-bit x86_64 executable
```

### 5. Ma trận đã verify (host Linux x86_64)

| Target | Builder | Ghi chú |
|--------|---------|--------|
| `x86_64-unknown-linux-gnu` | `cargo` | Smoke-test trên host |
| `x86_64-unknown-linux-musl` | `cross` (+ podman) | **Static-pie** — portable; smoke-test OK |
| `aarch64-unknown-linux-gnu` | `cross` (+ podman) | Dynamic aarch64; chỉ build trên x86_64 |
| `aarch64-unknown-linux-musl` | `cross` (+ podman) | **Static** aarch64; chỉ build |
| `x86_64-pc-windows-gnu` | `cross` (+ podman) | `.exe` x86-64; chỉ build |
| `aarch64-pc-windows-gnullvm` | `cargo-zigbuild` + zig 0.13 | `.exe` ARM64; chỉ build |
| `aarch64-apple-darwin` | `cargo-zigbuild` + zig 0.13 + MacOSX11.3 SDK | Mach-O arm64; chỉ build |
| `x86_64-apple-darwin` | `cargo-zigbuild` + zig 0.13 + MacOSX11.3 SDK | Mach-O x86_64; chỉ build |

Tên artifact sau khi chạy multi script (ví dụ):

```text
dist/content-sync-v0.1.0-x86_64-unknown-linux-gnu
dist/content-sync-v0.1.0-x86_64-unknown-linux-musl
dist/content-sync-v0.1.0-aarch64-unknown-linux-gnu
dist/content-sync-v0.1.0-aarch64-unknown-linux-musl
dist/content-sync-v0.1.0-x86_64-pc-windows-gnu.exe
dist/content-sync-v0.1.0-aarch64-pc-windows-gnullvm.exe
dist/content-sync-v0.1.0-aarch64-apple-darwin
dist/content-sync-v0.1.0-x86_64-apple-darwin
dist/SHA256SUMS.txt
```

### 6. Xử lý lỗi thường gặp

| Hiện tượng | Cách xử lý |
|------------|------------|
| macOS link: `unable to find framework 'CoreFoundation'` | Cài MacOSX SDK và set `SDKROOT` (mục 2.C) |
| macOS link hỏng với Zig 0.14 | Chuyển symlink sang **Zig 0.13.x** |
| `cross` fallback về cargo host | Cài podman/docker và set `CROSS_CONTAINER_ENGINE` |
| Lần `cross` đầu rất chậm | Bình thường — đang pull `ghcr.io/cross-rs/<target>:main` |
| Smoke test skipped | Đúng kỳ vọng với arch lạ / macOS / Windows trên Linux |
| `zig: command not found` | Thêm `~/.local/bin` vào `PATH`, symlink trỏ đúng Zig 0.13 |

## Container image (đẩy lên registry)

Script: [`scripts/registry-image-push.sh`](./scripts/registry-image-push.sh)  
Dockerfile: [`docker/Dockerfile`](./docker/Dockerfile) (Alpine + binary **linux musl** static)

### Pipeline

1. Build binary musl qua [`scripts/build-release-multi.sh`](./scripts/build-release-multi.sh) (`x86_64` + `aarch64` mặc định)
2. Stage vào `docker/binaries/{amd64,arm64}/`
3. Build image multi-arch (ưu tiên **Podman**, không có thì Docker/buildx)
4. Login + push tag lên một hoặc nhiều registry

### Runtime image

| Mục | Giá trị |
|-----|---------|
| Base | `docker.io/library/alpine:latest` |
| Binary | Static musl `content-sync` |
| CMD mặc định | `serve --bind 0.0.0.0:8080 --no-log` |
| Port | **8080** (phù hợp Cloud Run) |
| Volume | `/data` (= `HOME` → config tại `/data/.content-sync`) |
| User | non-root `appsync` (uid 1000) |

### Engine

- **Mặc định (`--engine auto`):** ưu tiên **Podman**, sau đó Docker
- Ép: `--engine podman` hoặc `--engine docker` (hoặc `CONTAINER_ENGINE=…`)

**Podman multi-arch** (script tự làm đúng thứ tự):

1. `podman manifest create <name>`
2. `podman build --platform linux/amd64 --manifest <name> …`
3. `podman build --platform linux/arm64 --manifest <name> …` (`aarch64` → `arm64`)
4. `podman manifest push --all <name> docker://…`

### Lệnh thường dùng

```bash
# Xem help đầy đủ
scripts/registry-image-push.sh --help

# Chỉ build local (arch host), không push
scripts/registry-image-push.sh --no-push --load

# Dùng sẵn binary dist/*-linux-musl
scripts/registry-image-push.sh --skip-binary-build --no-push --load

# Dry-run (in plan, không thực thi)
scripts/registry-image-push.sh --to dockerhub --dry-run --skip-binary-build
```

Chạy image:

```bash
podman run --rm -p 8080:8080 -v content-sync-data:/data content-sync:0.1.0
# Web UI → http://127.0.0.1:8080
```

### Push lên registry

Nên dùng biến môi trường cho secret (tránh lưu token trong history). Tag mặc định: **version Cargo** + `latest`.

#### Docker Hub

```bash
export DOCKERHUB_USER=myuser
export DOCKERHUB_TOKEN=***
scripts/registry-image-push.sh --to dockerhub --image content-sync
# → docker.io/myuser/content-sync:<version>
```

#### GitHub Container Registry (GHCR)

```bash
export GHCR_USER=myuser
export GHCR_TOKEN=ghp_***           # PAT: read:packages, write:packages
scripts/registry-image-push.sh --to ghcr --image content-sync
# → ghcr.io/myuser/content-sync:<version>
```

#### Google Artifact Registry (nên dùng thay GCR legacy)

```bash
# Một lần: tạo Docker repo
gcloud artifacts repositories create content-sync \
  --repository-format=docker --location=us-central1 --project=my-proj

export GCP_PROJECT=my-proj
export GCP_REGION=us-central1
export GCP_REPOSITORY=content-sync
# Auth A — gcloud (dev)
gcloud auth login
gcloud auth configure-docker us-central1-docker.pkg.dev
# Auth B — service account JSON (CI)
# export GCP_SA_KEY_FILE=./sa-key.json

scripts/registry-image-push.sh --to gar --image content-sync
# → us-central1-docker.pkg.dev/my-proj/content-sync/content-sync:<version>
```

GCR cũ: `--to gcr --project my-proj` → `gcr.io/my-proj/content-sync:<version>`.

#### Quay.io / registry tùy chỉnh

```bash
export QUAY_USER=… QUAY_TOKEN=…
scripts/registry-image-push.sh --to quay

export REGISTRY_HOST=registry.example.com REGISTRY_USER=u REGISTRY_TOKEN=t
scripts/registry-image-push.sh --to custom --image org/content-sync
```

#### Nhiều hub một lần

```bash
scripts/registry-image-push.sh --to dockerhub,ghcr --skip-binary-build
```

### Cờ hữu ích

| Cờ | Ý nghĩa |
|----|---------|
| `--to HUB[,HUB…]` | `dockerhub`, `ghcr`, `gcr`, `gar`, `quay`, `custom` |
| `--engine auto\|podman\|docker` | Engine (mặc định auto → podman trước) |
| `--image NAME` | Tên image (mặc định `content-sync`) |
| `--tag TAG[,…]` | Ghi đè tag (mặc định `<version>,latest`) |
| `--platforms LIST` | Mặc định `linux/amd64,linux/arm64` |
| `--skip-binary-build` | Dùng sẵn `dist/*-linux-musl` |
| `--no-push` | Chỉ build |
| `--load` | Load vào engine local (single/host arch) |
| `--project` / `--region` / `--repository` | GCP / GAR |
| `--username` / `--token` / `--namespace` | Ghi đè credential |
| `--dry-run` | Chỉ in lệnh |

### Ghi chú Cloud Run

- Image đã bind **`0.0.0.0:8080`** kèm **`--no-log`** (khớp container port phổ biến trên Cloud Run).
- Mount volume tại **`/data`** nếu cần giữ config / file.
- Nút **Restart app** trên Web UI tự restart process trong container; trên Cloud Run có thể làm instance shutdown — môi trường managed nên **redeploy** khi cần thay đổi cấp process.

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
| `~/.content-sync/config-sqlite` | Auth tokens, connections, settings, cache, sessions (hậu tố `-sqlite`, không dùng `.sqlite` — thân thiện GCS) |
| `~/.content-sync/files/<name>/` | Watch dir mặc định theo connection (tạo khi dùng connection; không tạo sẵn cây thư mục rỗng) |
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
