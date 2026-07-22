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

Host-only release helper (fmt + check + build):

```bash
./scripts/build-release.sh
# → ./target/release/content-sync
```

## Multi-platform release builds

Script: [`scripts/build-release-multi.sh`](./scripts/build-release-multi.sh)

It always runs **`cargo fmt`** first, then builds release artifacts into **`dist/`**:

```text
dist/content-sync-v<version>-<target>[.exe]
dist/SHA256SUMS.txt
```

**Builder preference (per target):** host `cargo` → `cargo-zigbuild` + zig → `cross` + podman/docker.

Verified on a **Linux x86_64** host (see matrix below). Runtime smoke tests (`--version` / `--help`) run only when the binary can execute on the host; macOS / Windows / foreign-arch Linux are build-only.

### 1. Prerequisites (always)

```bash
# Rust stable + rustup targets as needed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup default stable
rustup component add rustfmt

# Optional but recommended for tools installed under ~/.local
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
```

### 2. Install tools (pick what you need)

#### A. `cross` + Podman/Docker — Linux cross + Windows x86_64

Used successfully for:

- `x86_64-unknown-linux-musl`
- `aarch64-unknown-linux-gnu`
- `aarch64-unknown-linux-musl`
- `x86_64-pc-windows-gnu`

```bash
# Install cross
cargo install cross --git https://github.com/cross-rs/cross

# Container engine (podman preferred on many hosts)
# Debian/Ubuntu example:
#   sudo apt-get install -y podman
# Or install Docker Engine.

export CROSS_CONTAINER_ENGINE=podman   # or docker
# First build pulls ghcr.io/cross-rs/<target>:main (~1–2 GB per image)
```

#### B. `cargo-zigbuild` + Zig **0.13.x** — macOS + Windows ARM64

Used successfully for:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `aarch64-pc-windows-gnullvm`

> **Important:** use **Zig 0.13.x**. Zig **0.14+** failed macOS link (SDK/sysroot path issues) in our tests.

```bash
# cargo-zigbuild
cargo install cargo-zigbuild

# Zig 0.13.0 (Linux x86_64 host example)
ZIG_VER=0.13.0
curl -fL "https://ziglang.org/download/${ZIG_VER}/zig-linux-x86_64-${ZIG_VER}.tar.xz" \
  -o /tmp/zig.tar.xz
mkdir -p "$HOME/.local"
tar -xJf /tmp/zig.tar.xz -C "$HOME/.local"
# Layout: ~/.local/zig-linux-x86_64-0.13.0/zig  (or rename as you like)
ln -sfn "$HOME/.local/zig-linux-x86_64-${ZIG_VER}/zig" "$HOME/.local/bin/zig"
# If you extracted to ~/.local/zig-0.13.0:
# ln -sfn "$HOME/.local/zig-0.13.0/zig" "$HOME/.local/bin/zig"

export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
zig version   # expect 0.13.0
```

#### C. MacOSX SDK — required for **apple-darwin** from Linux

macOS link needs Apple frameworks (`CoreFoundation`, `Security`, …). Download a prepackaged SDK:

```bash
mkdir -p "$HOME/.local/macos-sdk"
curl -fL \
  "https://github.com/joseluisq/macosx-sdks/releases/download/11.3/MacOSX11.3.sdk.tar.xz" \
  | tar -xJ -C "$HOME/.local/macos-sdk"

export SDKROOT="$HOME/.local/macos-sdk/MacOSX11.3.sdk"
# build-release-multi.sh auto-detects this path if SDKROOT is unset
ls "$SDKROOT/System/Library/Frameworks" | head
```

### 3. One-shot multi build (recommended)

```bash
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
export CROSS_CONTAINER_ENGINE=podman   # if using cross
# SDKROOT auto-detected from ~/.local/macos-sdk/MacOSX*.sdk when present

# See which builder each target would use
./scripts/build-release-multi.sh --list

# Full default matrix (skips targets that lack tooling)
./scripts/build-release-multi.sh

# Subset
./scripts/build-release-multi.sh --only x86_64-unknown-linux-musl,aarch64-apple-darwin

# Build only (no smoke tests)
./scripts/build-release-multi.sh --skip-test
```

### 4. Manual build per target (verified)

Assume you are in the repo root, PATH includes `~/.local/bin` and `~/.cargo/bin`, and `SDKROOT` is set when building macOS.

#### Linux x86_64 (host / glibc)

```bash
cargo build --release --target x86_64-unknown-linux-gnu
# or without --target on an x86_64-unknown-linux-gnu host:
# cargo build --release
# → target/x86_64-unknown-linux-gnu/release/content-sync  (or target/release/)
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
# Cannot run on x86_64 host without qemu-aarch64
file target/aarch64-unknown-linux-gnu/release/content-sync
# ELF 64-bit … ARM aarch64 … dynamically linked
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

#### Windows ARM64 (gnullvm) via zigbuild

```bash
# requires cargo-zigbuild + zig 0.13.x
rustup target add aarch64-pc-windows-gnullvm
cargo zigbuild --release --target aarch64-pc-windows-gnullvm
file target/aarch64-pc-windows-gnullvm/release/content-sync.exe
# PE32+ executable … ARM64
```

#### macOS Apple Silicon (aarch64) via zigbuild + SDK

```bash
export SDKROOT="$HOME/.local/macos-sdk/MacOSX11.3.sdk"
rustup target add aarch64-apple-darwin
cargo zigbuild --release --target aarch64-apple-darwin
file target/aarch64-apple-darwin/release/content-sync
# Mach-O 64-bit arm64 executable
```

#### macOS Intel (x86_64) via zigbuild + SDK

```bash
export SDKROOT="$HOME/.local/macos-sdk/MacOSX11.3.sdk"
rustup target add x86_64-apple-darwin
cargo zigbuild --release --target x86_64-apple-darwin
file target/x86_64-apple-darwin/release/content-sync
# Mach-O 64-bit x86_64 executable
```

### 5. Verified matrix (Linux x86_64 CI-style host)

| Target | Builder | Notes |
|--------|---------|--------|
| `x86_64-unknown-linux-gnu` | `cargo` | Smoke-tested on host |
| `x86_64-unknown-linux-musl` | `cross` (+ podman) | **Static-pie** — portable across distros; smoke-tested |
| `aarch64-unknown-linux-gnu` | `cross` (+ podman) | Dynamic aarch64; build-only on x86_64 |
| `aarch64-unknown-linux-musl` | `cross` (+ podman) | **Static** aarch64; build-only on x86_64 |
| `x86_64-pc-windows-gnu` | `cross` (+ podman) | `.exe` x86-64; build-only |
| `aarch64-pc-windows-gnullvm` | `cargo-zigbuild` + zig 0.13 | `.exe` ARM64; build-only |
| `aarch64-apple-darwin` | `cargo-zigbuild` + zig 0.13 + MacOSX11.3 SDK | Mach-O arm64; build-only |
| `x86_64-apple-darwin` | `cargo-zigbuild` + zig 0.13 + MacOSX11.3 SDK | Mach-O x86_64; build-only |

Example artifact names after the multi script:

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

### 6. Troubleshooting

| Symptom | Fix |
|---------|-----|
| macOS link: `unable to find framework 'CoreFoundation'` | Install MacOSX SDK and set `SDKROOT` (section 2.C) |
| macOS link broken with Zig 0.14 | Downgrade / switch symlink to **Zig 0.13.x** |
| `cross` falls back to host cargo | Install podman/docker and set `CROSS_CONTAINER_ENGINE` |
| First `cross` build is slow | Normal — pulls `ghcr.io/cross-rs/<target>:main` |
| Smoke test skipped | Expected for non-native arch / macOS / Windows on Linux |
| `zig: command not found` | Ensure `~/.local/bin` is on `PATH` and symlink points to Zig 0.13 |

## Container image (registry push)

Script: [`scripts/registry-image-push.sh`](./scripts/registry-image-push.sh)  
Dockerfile: [`docker/Dockerfile`](./docker/Dockerfile) (Alpine + static **linux musl** binary)

### What it does

1. Builds musl binaries via [`scripts/build-release-multi.sh`](./scripts/build-release-multi.sh) (`x86_64` + `aarch64` by default)
2. Stages them under `docker/binaries/{amd64,arm64}/`
3. Builds a multi-arch runtime image (Podman preferred, else Docker/buildx)
4. Logs in and pushes tags to one or more registries

### Runtime image

| Item | Value |
|------|--------|
| Base | `docker.io/library/alpine:latest` |
| Binary | Static musl `content-sync` |
| Default CMD | `serve --bind 0.0.0.0:8080 --no-log` |
| Port | **8080** (Cloud Run–friendly) |
| Volume | `/data` (= `HOME` → config at `/data/.content-sync`) |
| User | non-root `appsync` (uid 1000) |

### Engine

- **Default (`--engine auto`):** prefer **Podman**, then Docker
- Force: `--engine podman` or `--engine docker` (or `CONTAINER_ENGINE=…`)

### Common commands

```bash
# Full help
scripts/registry-image-push.sh --help

# Local image only (host arch), no push
scripts/registry-image-push.sh --no-push --load

# Reuse existing dist/*-linux-musl binaries
scripts/registry-image-push.sh --skip-binary-build --no-push --load

# Dry-run (print plan)
scripts/registry-image-push.sh --to dockerhub --dry-run --skip-binary-build
```

Run the image:

```bash
# Podman or Docker
podman run --rm -p 8080:8080 -v content-sync-data:/data content-sync:0.1.0
# Web UI → http://127.0.0.1:8080
```

### Push to registries

Prefer env vars for secrets (not shell history). Tags default to **Cargo version** + `latest`.

#### Docker Hub

```bash
export DOCKERHUB_USER=myuser
export DOCKERHUB_TOKEN=***          # access token
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

#### Google Artifact Registry (recommended over legacy GCR)

```bash
# One-time: create a Docker repo
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

Legacy GCR: `--to gcr --project my-proj` → `gcr.io/my-proj/content-sync:<version>`.

#### Quay.io / custom registry

```bash
export QUAY_USER=… QUAY_TOKEN=…
scripts/registry-image-push.sh --to quay

export REGISTRY_HOST=registry.example.com REGISTRY_USER=u REGISTRY_TOKEN=t
scripts/registry-image-push.sh --to custom --image org/content-sync
```

#### Several hubs at once

```bash
scripts/registry-image-push.sh --to dockerhub,ghcr --skip-binary-build
```

### Useful flags

| Flag | Meaning |
|------|---------|
| `--to HUB[,HUB…]` | `dockerhub`, `ghcr`, `gcr`, `gar`, `quay`, `custom` |
| `--engine auto\|podman\|docker` | Container engine (default: auto → podman first) |
| `--image NAME` | Image name (default: `content-sync`) |
| `--tag TAG[,…]` | Override tags (default: `<version>,latest`) |
| `--platforms LIST` | Default `linux/amd64,linux/arm64` |
| `--skip-binary-build` | Use existing `dist/*-linux-musl` |
| `--no-push` | Build only |
| `--load` | Load into local engine (single/host arch) |
| `--project` / `--region` / `--repository` | GCP / GAR |
| `--username` / `--token` / `--namespace` | Override credentials |
| `--dry-run` | Print actions only |

### Cloud Run notes

- Image already binds **`0.0.0.0:8080`** with **`--no-log`** (matches common Cloud Run container port).
- Mount a volume at **`/data`** if you need persistent config / files.
- **Web UI “Restart app”** self-restarts the process inside the container; on Cloud Run that can stop the instance — prefer **redeploy** for process-level changes in managed environments.

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
| `content-sync sync` | One-shot sync, then exit (Dashboard “Sync now”) |
| `content-sync status` | Show configuration (includes background PID status) |
| `content-sync logs` | Recent sync logs (`--limit`, `--level`) |
| `content-sync settings show` / `settings set …` | View / update settings |
| `content-sync export` | Export system config to `export.content.sync.YYYY-MM-DD.HH-MM-SS.json` |
| `content-sync export -o backup.json` | Export to a custom path |
| `content-sync import <file>` | Import config (prompts; use `-y` to skip) |
| `content-sync token create/show/list/set/delete` | Manage auth tokens |
| `content-sync connection add/list/show/set/toggle/clone/test/delete` | Manage DB connections (name or id) |
| `content-sync file list/show/write/delete` | Manage watched files (name or id for connection) |

**Web UI ↔ CLI parity**

| Web | CLI |
|-----|-----|
| Dashboard / Sync now / logs | `sync`, `logs`, `status` |
| Files | `file list\|show\|write\|delete` |
| Connections (add, on/off, test, clone, edit, delete) | `connection add\|toggle\|test\|clone\|set\|delete\|show\|list` |
| Auth Tokens | `token create\|list\|set\|delete\|show` |
| Settings + export/import | `settings show\|set`, `export`, `import` |

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
