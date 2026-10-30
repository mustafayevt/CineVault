# CineVault browser server.
#
# Two builder stages keep the Rust toolchain and the JavaScript toolchain out of the runtime
# image: one compiles `cinevault-server` (and the `cinevault` CLI), the other builds the React
# interface. The runtime is a slim Debian with the binaries, the interface and a small entrypoint
# that maps PUID/PGID and drops to a non-root user.
#
#   docker build -t cinevault-server .
#   docker run --rm -p 8787:8787 -v "$PWD/data:/data" -v /path/to/media:/media cinevault-server
#
# The version is read by scripts/check-version.mjs, so keep it in step with Cargo.toml.
ARG CINEVAULT_VERSION=1.1.1

# ------------------------------------------------------------------------------------------------
# Builder 1: the server and CLI in the release profile.
# ------------------------------------------------------------------------------------------------
FROM rust:1.99-bookworm AS rust-builder

# OpenSSL and libdbus are linked through native-tls and the Linux keyring backend. SQLite, WebP
# and liblzma are built from source and need a C compiler, which the Rust image already has.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        libdbus-1-dev \
        libssl-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY . .

# The server is the image's program; the CLI ships beside it as it does in the packages.
RUN cargo build --release --locked -p cv-web --bin cinevault-server \
    && cargo build --release --locked -p cinevault-cli --bin cinevault

# ------------------------------------------------------------------------------------------------
# Builder 2: the interface.
# ------------------------------------------------------------------------------------------------
FROM oven/bun:1.3.14 AS ui-builder

WORKDIR /src/ui
COPY ui/package.json ui/bun.lock ./
RUN bun install --frozen-lockfile

# The UI imports ../CHANGELOG.md (its release-notes page), so the repository root is one level up.
COPY ui/ ./
COPY CHANGELOG.md /src/CHANGELOG.md
RUN bun run build

# ------------------------------------------------------------------------------------------------
# Runtime: Debian slim, non-root, the server binary and the built interface.
# ------------------------------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

ARG CINEVAULT_VERSION

LABEL org.opencontainers.image.title="CineVault server" \
      org.opencontainers.image.description="CineVault movie and TV library server for a browser" \
      org.opencontainers.image.source="https://github.com/mustafayevt/CineVault" \
      org.opencontainers.image.version="${CINEVAULT_VERSION}" \
      org.opencontainers.image.licenses="Apache-2.0"

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        libdbus-1-3 \
        libssl3 \
        passwd \
        util-linux \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 1000 cinevault \
    && useradd --uid 1000 --gid cinevault --home-dir /data --no-create-home \
        --shell /usr/sbin/nologin cinevault \
    && install -d -o cinevault -g cinevault -m 0755 /data \
    && install -d -o cinevault -g cinevault -m 0755 /media \
    && install -d -m 0755 /usr/share/cinevault

COPY --from=rust-builder /src/target/release/cinevault-server /usr/local/bin/cinevault-server
COPY --from=rust-builder /src/target/release/cinevault /usr/local/bin/cinevault
COPY --from=ui-builder /src/ui/dist/ /usr/share/cinevault/ui/
COPY packaging/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod 0755 /usr/local/bin/cinevault-server /usr/local/bin/cinevault \
        /usr/local/bin/entrypoint.sh

# The server reads the data folder from CINEVAULT_DATA_DIR. The bind and port are also passed as
# flags by the entrypoint, so the image works whether or not the server reads them itself.
ENV CINEVAULT_DATA_DIR=/data \
    CINEVAULT_WEB_BIND=0.0.0.0 \
    CINEVAULT_WEB_PORT=8787 \
    HOME=/data \
    RUST_LOG=info

WORKDIR /data
VOLUME ["/data", "/media"]
EXPOSE 8787

# The server handles SIGINT for a clean stop (jobs paused, owner file removed). Docker would send
# SIGTERM otherwise, which would kill it without that cleanup.
STOPSIGNAL SIGINT

HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8787/healthz || exit 1

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
