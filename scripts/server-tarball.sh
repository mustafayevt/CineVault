#!/usr/bin/env bash
# Builds the server tarball for hosts without a package manager:
#
#   bun run server:tarball
#
# On Linux the archive is cinevault-server-<version>-linux-<arch>.tar.gz. On macOS it is built for
# local testing only and named -macos-<arch>: the server release artifact is Linux-only.
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if ! command -v cargo >/dev/null 2>&1 && [ -x "${HOME:-}/.cargo/bin/cargo" ]; then
    PATH="${HOME}/.cargo/bin:${PATH}"
    export PATH
fi

for tool in bun cargo tar; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "server-tarball: missing $tool; install bun and Rust first." >&2
        exit 1
    fi
done

version="$(awk '
    /^\[workspace\.package\]/ { table = 1; next }
    /^\[/ { table = 0 }
    table && /^version[[:space:]]*=/ {
        value = $0
        sub(/^[^=]*=[[:space:]]*"/, "", value)
        sub(/".*$/, "", value)
        print value
        exit
    }
' Cargo.toml)"
if [ -z "$version" ]; then
    echo "server-tarball: could not read [workspace.package] version from Cargo.toml." >&2
    exit 1
fi

case "$(uname -s)" in
    Linux) os=linux ;;
    Darwin) os=macos ;;
    *) os="$(uname -s | tr '[:upper:]' '[:lower:]')" ;;
esac
case "$(uname -m)" in
    x86_64|amd64) arch=amd64 ;;
    arm64|aarch64) arch=arm64 ;;
    *)
        echo "server-tarball: unsupported architecture '$(uname -m)' (expected x86_64 or arm64)." >&2
        exit 1
        ;;
esac
if [ "$os" != "linux" ]; then
    echo "server-tarball: building a $os archive for local testing; release artifacts are Linux only." >&2
fi

echo "CineVault server $version ($os-$arch)"
echo "Building the interface..."
(
    cd ui
    bun install --frozen-lockfile
    bun run build
)

echo "Building the release binaries..."
cargo build --release --locked -p cv-web --bin cinevault-server
cargo build --release --locked -p cinevault-cli --bin cinevault

for binary in target/release/cinevault-server target/release/cinevault; do
    if [ ! -x "$binary" ]; then
        echo "server-tarball: missing $binary after the build." >&2
        exit 1
    fi
done
if [ ! -f ui/dist/index.html ]; then
    echo "server-tarball: the UI build did not produce ui/dist/index.html." >&2
    exit 1
fi

out="$root/target/server-tarball"
rm -rf "$out"
work="$out/.work"
name="cinevault-server-${version}"
stage="$work/$name"
mkdir -p "$stage/bin" "$stage/ui"
trap 'rm -rf "$work"' EXIT

install -m 0755 target/release/cinevault-server "$stage/bin/cinevault-server"
install -m 0755 target/release/cinevault "$stage/bin/cinevault"
cp -a ui/dist/. "$stage/ui/"
install -m 0644 packaging/cinevault-server.service "$stage/cinevault-server.service"
install -m 0644 LICENSE "$stage/LICENSE"
install -m 0644 NOTICE "$stage/NOTICE"

cat > "$stage/run.sh" <<'EOF'
#!/bin/sh
# Starts the server from this folder. Data goes to ./data unless CINEVAULT_DATA_DIR says otherwise.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
data_dir="${CINEVAULT_DATA_DIR:-$here/data}"
mkdir -p "$data_dir"
export CINEVAULT_DATA_DIR="$data_dir"
bind="${CINEVAULT_WEB_BIND:-127.0.0.1}"
port="${CINEVAULT_WEB_PORT:-8787}"
exec "$here/bin/cinevault-server" --ui "$here/ui" --bind "$bind" --port "$port" "$@"
EOF
chmod 0755 "$stage/run.sh"

cat > "$stage/README.txt" <<EOF
CineVault server $version ($os-$arch)

Run it:
  ./run.sh

Then open http://127.0.0.1:8787/ and create the password on the welcome page.
To reach it from another machine:
  CINEVAULT_WEB_BIND=0.0.0.0 ./run.sh

Data is kept in ./data by default; set CINEVAULT_DATA_DIR to put it elsewhere.
The library folders you add must be writable by the user running the server.
The systemd unit in this folder is an example for a system-wide install; see
packaging/README.md in the CineVault repository for the install paths and permissions.
EOF

archive="$out/${name}-${os}-${arch}.tar.gz"
tar -czf "$archive" -C "$work" "$name"
echo "Built $archive"
