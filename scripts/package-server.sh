#!/usr/bin/env bash
# Builds the Linux server packages: cinevault-server_<version>_<arch>.deb and .rpm.
#
#   bun run server:package
#
# Both packages install the same tree:
#   /usr/local/bin/cinevault-server
#   /usr/local/bin/cinevault
#   /usr/share/cinevault/ui
#   /usr/lib/systemd/system/cinevault-server.service
#
# The desktop packages stay named `cinevault`; these are `cinevault-server`, so both can be
# installed on one machine. Linux only: the server ships for Linux, and dpkg/rpm cannot describe a
# macOS build.
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if [ "$(uname -s)" != "Linux" ]; then
    echo "package-server: the server packages are built on Linux only (this is $(uname -s))." >&2
    echo "Use the release workflow, a Linux machine, WSL, or 'bun run server:tarball' for a local archive." >&2
    exit 1
fi

# Cargo is commonly installed by rustup outside the default PATH on developer machines.
if ! command -v cargo >/dev/null 2>&1 && [ -x "${HOME:-}/.cargo/bin/cargo" ]; then
    PATH="${HOME}/.cargo/bin:${PATH}"
    export PATH
fi

missing=()
for tool in bun cargo dpkg-deb rpmbuild; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        missing+=("$tool")
    fi
done
if [ "${#missing[@]}" -gt 0 ]; then
    echo "package-server: missing ${missing[*]}." >&2
    echo "Install the package tools with: sudo apt-get install -y dpkg-dev rpm" >&2
    echo "(or: sudo dnf install -y dpkg rpm-build), then install Rust and bun." >&2
    exit 1
fi

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
    echo "package-server: could not read [workspace.package] version from Cargo.toml." >&2
    exit 1
fi

deb_arch="$(dpkg --print-architecture)"
case "$deb_arch" in
    amd64) rpm_arch="x86_64" ;;
    arm64) rpm_arch="aarch64" ;;
    *)
        echo "package-server: unsupported architecture '$deb_arch' (expected amd64 or arm64)." >&2
        exit 1
        ;;
esac

echo "CineVault server $version ($deb_arch)"
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
        echo "package-server: missing $binary after the build." >&2
        exit 1
    fi
done
if [ ! -f ui/dist/index.html ]; then
    echo "package-server: the UI build did not produce ui/dist/index.html." >&2
    exit 1
fi

out="$root/target/server-package"
work="$out/.work"
rm -rf "$out"
mkdir -p "$work"
trap 'rm -rf "$work"' EXIT

stage="$work/root"
mkdir -p \
    "$stage/usr/local/bin" \
    "$stage/usr/share/cinevault/ui" \
    "$stage/usr/share/doc/cinevault-server" \
    "$stage/usr/lib/systemd/system"

install -m 0755 target/release/cinevault-server "$stage/usr/local/bin/cinevault-server"
install -m 0755 target/release/cinevault "$stage/usr/local/bin/cinevault"
cp -a ui/dist/. "$stage/usr/share/cinevault/ui/"
install -m 0644 packaging/cinevault-server.service "$stage/usr/lib/systemd/system/cinevault-server.service"
install -m 0644 LICENSE "$stage/usr/share/doc/cinevault-server/copyright"
install -m 0644 NOTICE "$stage/usr/share/doc/cinevault-server/NOTICE"

# ------------------------------------------------------------------------------------------------
# Debian package
# ------------------------------------------------------------------------------------------------
deb_root="$work/deb"
mkdir -p "$deb_root/DEBIAN"
cp -a "$stage/." "$deb_root/"

installed_size="$(du -sk "$deb_root/usr" | cut -f1)"
cat > "$deb_root/DEBIAN/control" <<EOF
Package: cinevault-server
Version: $version
Architecture: $deb_arch
Maintainer: CineVault maintainers <mustafayevt@users.noreply.github.com>
Installed-Size: $installed_size
Depends: libc6, libgcc-s1, libssl3, libdbus-1-3, ca-certificates, passwd
Recommends: systemd
Section: video
Priority: optional
Homepage: https://github.com/mustafayevt/CineVault
Description: CineVault movie and TV library server
 A headless server for the CineVault browser interface. It serves the
 library, the management commands and the built interface over HTTP.
 .
 The cinevault command-line tool is included.
EOF

cat > "$deb_root/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if ! getent group cinevault >/dev/null 2>&1; then
    groupadd --system cinevault
fi
if ! getent passwd cinevault >/dev/null 2>&1; then
    useradd --system --gid cinevault --home-dir /srv/cinevault \
        --no-create-home --shell /usr/sbin/nologin cinevault
fi
install -d -m 0750 -o cinevault -g cinevault /srv/cinevault
if command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload >/dev/null 2>&1 || true
fi
exit 0
EOF

cat > "$deb_root/DEBIAN/prerm" <<'EOF'
#!/bin/sh
set -e
if [ "${1:-}" = remove ] && command -v systemctl >/dev/null 2>&1; then
    systemctl stop cinevault-server.service >/dev/null 2>&1 || true
    systemctl disable cinevault-server.service >/dev/null 2>&1 || true
fi
exit 0
EOF

cat > "$deb_root/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
if command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload >/dev/null 2>&1 || true
fi
exit 0
EOF

chmod 0755 "$deb_root/DEBIAN/postinst" "$deb_root/DEBIAN/prerm" "$deb_root/DEBIAN/postrm"
# Keep local edits to the unit when the package is upgraded.
printf '%s\n' /usr/lib/systemd/system/cinevault-server.service > "$deb_root/DEBIAN/conffiles"

deb_file="$out/cinevault-server_${version}_${deb_arch}.deb"
dpkg-deb --root-owner-group --build "$deb_root" "$deb_file" >/dev/null
echo "Built $deb_file"

# ------------------------------------------------------------------------------------------------
# RPM package
# ------------------------------------------------------------------------------------------------
rpm_top="$work/rpm"
mkdir -p "$rpm_top/BUILD" "$rpm_top/BUILDROOT" "$rpm_top/RPMS" "$rpm_top/SOURCES" \
    "$rpm_top/SPECS" "$rpm_top/SRPMS"
spec="$rpm_top/SPECS/cinevault-server.spec"

cat > "$spec" <<EOF
Name:           cinevault-server
Version:        $version
Release:        1
Summary:        CineVault movie and TV library server
License:        Apache-2.0
URL:            https://github.com/mustafayevt/CineVault
BuildArch:      $rpm_arch
Requires:       glibc, glibc-common, libgcc, openssl-libs, dbus-libs, ca-certificates, shadow-utils

%description
A headless server for the CineVault browser interface. It serves the library,
the management commands and the built interface over HTTP.
The cinevault command-line tool is included.

%prep

%build

%install
rm -rf %{buildroot}
mkdir -p %{buildroot}
cp -a "%{_cinevault_stage}/." "%{buildroot}/"

%pre
if ! getent group cinevault >/dev/null 2>&1; then
    groupadd --system cinevault
fi
if ! getent passwd cinevault >/dev/null 2>&1; then
    useradd --system --gid cinevault --home-dir /srv/cinevault \\
        --no-create-home --shell /usr/sbin/nologin cinevault
fi
exit 0

%post
install -d -m 0750 -o cinevault -g cinevault /srv/cinevault || :
if command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload >/dev/null 2>&1 || :
fi
exit 0

%preun
if [ "\${1:-}" = 0 ]; then
    if command -v systemctl >/dev/null 2>&1; then
        systemctl stop cinevault-server.service >/dev/null 2>&1 || :
        systemctl disable cinevault-server.service >/dev/null 2>&1 || :
    fi
fi
exit 0

%postun
if command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload >/dev/null 2>&1 || :
fi
exit 0

%files
%attr(0755,root,root) /usr/local/bin/cinevault-server
%attr(0755,root,root) /usr/local/bin/cinevault
/usr/share/cinevault
/usr/share/doc/cinevault-server/copyright
/usr/share/doc/cinevault-server/NOTICE
%config(noreplace) %attr(0644,root,root) /usr/lib/systemd/system/cinevault-server.service
EOF

rpmbuild -bb \
    --define "_topdir $rpm_top" \
    --define "_cinevault_stage $stage" \
    "$spec" >/dev/null

rpm_file="$(find "$rpm_top/RPMS" -type f -name '*.rpm' | head -n 1)"
if [ -z "$rpm_file" ]; then
    echo "package-server: rpmbuild produced no package." >&2
    exit 1
fi
cp "$rpm_file" "$out/"
echo "Built $out/$(basename "$rpm_file")"
