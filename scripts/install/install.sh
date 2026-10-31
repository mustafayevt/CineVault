#!/bin/sh
# CineVault browser mode (beta): install, update or remove the CineVault server.
#
#   curl -fsSL https://mustafayevt.github.io/CineVault/install.sh | sh                   install or update
#   curl -fsSL https://mustafayevt.github.io/CineVault/install.sh | sh -s -- uninstall   remove it
#   curl -fsSL https://mustafayevt.github.io/CineVault/install.sh | sh -s -- --docker    use Docker
#
# On Linux it installs the cinevault-server package from the latest GitHub release (.deb on Debian
# and Ubuntu, .rpm on Fedora, RHEL and openSUSE, the tarball elsewhere), checks its SHA-256, asks
# which movie and TV folders CineVault may use, makes the key that protects your provider keys and
# starts the cinevault-server systemd service, which comes back after a restart. It needs sudo for
# that. On macOS, and on Linux with --docker, it runs the ghcr.io/mustafayevt/cinevault image in
# Docker instead. Run it again to update, change the folders or remove CineVault; your library,
# settings and password are kept. It sends nothing anywhere: it only downloads from github.com and
# ghcr.io (and from get.docker.com, only if you ask it to install Docker).
#
# Without questions (all optional except CINEVAULT_MEDIA on a first install):
#   CINEVAULT_YES=1                         accept every default, never ask
#   CINEVAULT_MEDIA="/srv/movies;/srv/tv"   library folders, separated by ";"
#   CINEVAULT_PORT=8787                     the port to open in the browser
#   CINEVAULT_NO_BROWSER=1                  do not open a browser at the end
#   CINEVAULT_ACTION=install|update|folders|uninstall   (or pass it as an argument)
#   CINEVAULT_DOCKER=1                      use Docker on Linux too (or pass --docker)
#   CINEVAULT_VERSION=1.2.0                 a release instead of the latest (Linux packages)
#   CINEVAULT_DIR=~/cinevault               Docker: where the data folder and the key file go
#   CINEVAULT_IMAGE=ghcr.io/mustafayevt/cinevault:latest   Docker: the image
set -eu

REPO=mustafayevt/CineVault
SITE=https://mustafayevt.github.io/CineVault
NAME=cinevault
IMAGE="${CINEVAULT_IMAGE:-ghcr.io/mustafayevt/cinevault:latest}"
ACTION="${CINEVAULT_ACTION:-install}"
MODE=native
[ -z "${CINEVAULT_DOCKER:-}" ] || MODE=docker
for arg in "$@"; do
    case "$arg" in
        --docker) MODE=docker ;;
        *) ACTION=$arg ;;
    esac
done
YES="${CINEVAULT_YES:-}"
DOCKER=docker
SUDO=""
MEDIA=""

say() { printf '%s\n' "$*"; }
step() { printf '\n==> %s\n' "$*"; }
die() { printf '\nError: %s\n' "$*" >&2; exit 1; }

has_tty() { (exec </dev/tty) 2>/dev/null; }

# Questions are read from the terminal, because the script itself arrives on standard input.
ask() { # ask "Question" "default" -> prints the answer
    if [ -n "$YES" ]; then printf '%s' "$2"; return; fi
    has_tty || die "No terminal to ask questions on. Set CINEVAULT_YES=1 and the CINEVAULT_* variables (see the top of this script)."
    if [ -n "$2" ]; then printf '%s [%s]: ' "$1" "$2" >/dev/tty; else printf '%s: ' "$1" >/dev/tty; fi
    read -r answer </dev/tty || answer=""
    [ -n "$answer" ] || answer="$2"
    printf '%s' "$answer"
}
yes_no() { # yes_no "Question" y|n -> true when yes
    reply=$(ask "$1 (y/n)" "$2")
    case "$reply" in [Yy]*) return 0 ;; *) return 1 ;; esac
}

OS=$(uname -s)
case "$OS" in
    Linux) ;;
    Darwin) MODE=docker ;;
    *) die "This script is for Linux and macOS. On Windows, open PowerShell and run: irm $SITE/install.ps1 | iex" ;;
esac
command -v curl >/dev/null 2>&1 || die "curl is needed. Install it with your package manager and run this again."

# ---- shared -------------------------------------------------------------------------------------
lan_ip() {
    if [ "$OS" = Darwin ]; then
        iface=$(route -n get default 2>/dev/null | awk '/interface:/{print $2}')
        ipconfig getifaddr "${iface:-en0}" 2>/dev/null || true
    else
        ip route get 1.1.1.1 2>/dev/null | awk '{for (i = 1; i < NF; i++) if ($i == "src") { print $(i + 1); exit }}' ||
            true
    fi
}

port_free() { # true when nothing answers on the port
    rc=0
    curl -s -o /dev/null --connect-timeout 2 "http://127.0.0.1:$1/" || rc=$?
    [ "$rc" -eq 7 ]
}

pick_port() {
    PORT="${CINEVAULT_PORT:-8787}"
    if ! port_free "$PORT"; then
        p=$PORT
        until port_free "$p"; do p=$((p + 1)); [ "$p" -lt 8800 ] || die "No free port near $PORT. Set CINEVAULT_PORT."; done
        say "Port $PORT is in use, so CineVault will use port $p."
        PORT=$p
    fi
}

add_media() { # checks a folder and adds its full path to MEDIA
    folder=$(printf '%s' "$1" | sed "s/^[\"']//; s/[\"']\$//; s#/*\$##")
    case "$folder" in "~"*) folder="$HOME${folder#\~}" ;; esac
    [ -n "$folder" ] || folder=/
    if [ ! -d "$folder" ]; then say "  There is no folder $folder."; return 1; fi
    folder=$(cd "$folder" && pwd -P)
    MEDIA="${MEDIA:+$MEDIA
}$folder"
}

ask_media() { # fills MEDIA with the library folders
    MEDIA=""
    step "Your movie and TV folders"
    if [ -n "${CINEVAULT_MEDIA:-}" ]; then
        list=$(printf '%s\n' "$CINEVAULT_MEDIA" | tr ';' '\n')
        while IFS= read -r f; do
            [ -z "$f" ] || add_media "$f" || die "Fix CINEVAULT_MEDIA and run this again."
        done <<EOF_MEDIA
$list
EOF_MEDIA
    else
        [ -z "$YES" ] || die "Set CINEVAULT_MEDIA to your library folders, for example CINEVAULT_MEDIA=\"/srv/movies;/srv/tv\"."
        say "CineVault reads and renames the files in these folders, and writes artwork and NFO files next to them."
        example=/srv/movies
        [ "$OS" = Darwin ] && example=/Volumes/Media/Movies
        while :; do
            f=$(ask "Folder with your movies or TV shows (for example $example)" "")
            [ -n "$f" ] && add_media "$f" && break
        done
        while :; do
            f=$(ask "Another folder? Type its path, or press Enter to go on" "")
            [ -n "$f" ] || break
            add_media "$f" || true
        done
    fi
}

wait_url() { # waits until the server answers; $1 is a command that prints why it did not
    step "Waiting for CineVault to start"
    i=0
    until curl -fsS "http://127.0.0.1:$PORT/healthz" >/dev/null 2>&1; do
        i=$((i + 1))
        if [ "$i" -gt 45 ] || ! still_starting; then
            $1 >&2 || true
            die "CineVault did not start. The lines above say why."
        fi
        sleep 2
    done
}

finish() { # finish fresh|update "how to restart" "folder lines"
    url="http://localhost:$PORT/"
    ip=$(lan_ip)
    say ""
    say "CineVault is running."
    say "  On this computer:      $url"
    [ -z "$ip" ] || say "  From another device:   http://$ip:$PORT/"
    say ""
    if [ "$1" = fresh ]; then
        say "First visit: choose a password on the welcome page (from any device at home)."
        say "Then add your folders in setup:"
    else
        say "Sign in with your password. Your folders, as CineVault sees them:"
    fi
    say "$3"
    say ""
    say "Run this command again to update CineVault, change its folders or remove it."
    if [ -z "${CINEVAULT_NO_BROWSER:-}" ]; then
        if [ "$OS" = Darwin ]; then open "$url" >/dev/null 2>&1 || true
        elif [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] && command -v xdg-open >/dev/null 2>&1; then
            xdg-open "$url" >/dev/null 2>&1 || true
        fi
    fi
}

menu() { # what to do with an existing install
    say "CineVault is already installed."
    say "  1  Update to the newest version (keeps your library, settings and password)"
    say "  2  Change which movie and TV folders CineVault may use"
    say "  3  Remove CineVault (keeps your data unless you ask)"
    say "  4  Stop here"
    case "$(ask "Type a number" 1)" in
        1) ACTION=update ;;
        2) ACTION=folders ;;
        3) ACTION=uninstall ;;
        *) say "Nothing changed."; exit 0 ;;
    esac
}

# ==== Linux package and systemd service (the default on Linux) =====================================
UNIT=cinevault-server
DROPIN_DIR=/etc/systemd/system/cinevault-server.service.d
DROPIN=$DROPIN_DIR/cinevault-install.conf
KEY=/etc/cinevault/cinevault.key
DATA=/srv/cinevault

as_root() { $SUDO "$@"; }

need_root() {
    if [ "$(id -u)" -ne 0 ]; then
        command -v sudo >/dev/null 2>&1 || die "Run this as root (sudo is not installed)."
        SUDO=sudo
        say "Installing a service needs administrator rights; sudo may ask for your password."
        if has_tty; then
            # shellcheck disable=SC2024 # the redirect is for sudo's own password prompt
            sudo -v </dev/tty || die "sudo was refused."
        else
            sudo -n true 2>/dev/null || die "sudo needs a password and there is no terminal to ask for it."
        fi
    fi
}

n_installed() { [ -x /usr/local/bin/cinevault-server ]; }
has_systemd() { [ -d /run/systemd/system ]; }

n_check_platform() {
    docker_hint="Use Docker instead: curl -fsSL $SITE/install.sh | sh -s -- --docker"
    [ "$(uname -m)" = x86_64 ] || die "The CineVault server is built for 64-bit Intel and AMD (x86_64) so far; this computer is $(uname -m)."
    glibc=$(ldd --version 2>/dev/null | head -n 1 | awk '{print $NF}') || glibc=""
    minor=${glibc#2.}
    minor=${minor%%.*}
    case "$glibc" in
        2.*) [ "$minor" -ge 34 ] 2>/dev/null ||
            die "This Linux is too old for the CineVault packages (glibc $glibc; 2.34 or newer is needed: Ubuntu 22.04, Debian 12, Fedora 36 or later). $docker_hint" ;;
        *) die "This Linux does not use glibc, which the CineVault packages need. $docker_hint" ;;
    esac
}

n_kind() {
    if command -v apt-get >/dev/null 2>&1 && command -v dpkg >/dev/null 2>&1; then echo deb
    elif command -v dnf >/dev/null 2>&1 || command -v yum >/dev/null 2>&1 || command -v zypper >/dev/null 2>&1; then echo rpm
    else echo tar; fi
}

n_download() { # sets VERSION, KIND and PKG (a verified file in a temporary folder)
    if [ -n "${CINEVAULT_VERSION:-}" ]; then
        VERSION=${CINEVAULT_VERSION#v}
    else
        latest=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest") ||
            die "GitHub could not be reached. Check the internet connection and try again."
        VERSION=${latest##*/v}
    fi
    KIND=$(n_kind)
    case "$KIND" in
        deb) file="cinevault-server_${VERSION}_amd64.deb" ;;
        rpm) file="cinevault-server-${VERSION}-1.x86_64.rpm" ;;
        *) file="cinevault-server-${VERSION}-linux-amd64.tar.gz" ;;
    esac
    TMP=$(mktemp -d)
    trap 'rc=$?; rm -rf "$TMP"; [ "$rc" -eq 0 ] || echo "The installer stopped (error $rc) at the step shown above." >&2' EXIT
    chmod 755 "$TMP"
    base="https://github.com/$REPO/releases/download/v$VERSION"
    step "Downloading CineVault $VERSION ($file)"
    curl -fL --progress-bar -o "$TMP/$file" "$base/$file" || die "$file could not be downloaded from $base."
    curl -fsSL -o "$TMP/SHA256SUMS" "$base/SHA256SUMS-Linux.txt" || die "The checksums could not be downloaded."
    expected=$(awk -v f="$file" '$2 == f || $2 == "*" f { print $1 }' "$TMP/SHA256SUMS")
    actual=$(sha256sum "$TMP/$file" | awk '{ print $1 }')
    if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then die "The download does not match its published SHA-256 checksum. Nothing was installed."; fi
    chmod 644 "$TMP/$file"
    PKG="$TMP/$file"
}

n_install_package() {
    step "Installing the cinevault-server package"
    case "$KIND" in
        deb) as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "$PKG" ;;
        rpm)
            if command -v dnf >/dev/null 2>&1; then as_root dnf install -y "$PKG"
            elif command -v yum >/dev/null 2>&1; then as_root yum install -y "$PKG"
            else as_root zypper --non-interactive install --allow-unsigned-rpm "$PKG"; fi ;;
        *)
            # No package manager this script knows: the tarball, laid out as the packages are.
            as_root tar -xzf "$PKG" -C "$TMP"
            src="$TMP/cinevault-server-$VERSION"
            as_root install -d /usr/local/bin /etc/systemd/system
            as_root install -m 0755 "$src/bin/cinevault-server" "$src/bin/cinevault" /usr/local/bin/
            as_root rm -rf /usr/share/cinevault/ui
            as_root mkdir -p /usr/share/cinevault
            as_root cp -R "$src/ui" /usr/share/cinevault/ui
            as_root install -m 0644 "$src/cinevault-server.service" /etc/systemd/system/cinevault-server.service
            getent group cinevault >/dev/null 2>&1 || as_root groupadd --system cinevault
            getent passwd cinevault >/dev/null 2>&1 ||
                as_root useradd --system --gid cinevault --home-dir "$DATA" --no-create-home --shell /usr/sbin/nologin cinevault
            as_root install -d -m 0750 -o cinevault -g cinevault "$DATA" ;;
    esac
}

n_read_config() { # PORT, MEDIA and RUN_USER from the installer's systemd drop-in
    PORT=$(sed -n 's/^Environment=CINEVAULT_WEB_PORT=//p' "$DROPIN" 2>/dev/null)
    PORT=${PORT:-8787}
    MEDIA=$(sed -n 's/^Environment="CINEVAULT_WEB_ROOTS=\(.*\)"$/\1/p' "$DROPIN" 2>/dev/null | sed 's/%%/%/g' | tr ':' '\n')
    RUN_USER=$(sed -n 's/^User=//p' "$DROPIN" 2>/dev/null)
}

n_configure() { # the key, and a drop-in with the account, the folders and the port
    if [ -z "${RUN_USER:-}" ]; then
        # The service runs as the person installing it, who usually owns the media; as root, it
        # keeps the package's own cinevault account.
        RUN_USER=${SUDO_USER:-$(id -un)}
        [ "$RUN_USER" != root ] || RUN_USER=cinevault
    fi
    run_group=$(id -gn "$RUN_USER")
    as_root install -d -m 0755 /etc/cinevault
    if ! as_root test -s "$KEY"; then
        # shellcheck disable=SC2016 # expanded by the inner shell
        as_root sh -c 'umask 077; if command -v openssl >/dev/null 2>&1; then openssl rand -base64 32; else head -c 32 /dev/urandom | base64; fi > "$1"' sh "$KEY"
    fi
    roots=$(printf '%s\n' "$MEDIA" | paste -sd ':' - | sed 's/%/%%/g')
    protect_home=""
    case ":$roots" in *:/home/* | *:/home | *:/root* | *:/run/user/*) protect_home="ProtectHome=false" ;; esac
    as_root install -d -m 0755 "$DROPIN_DIR"
    # shellcheck disable=SC2016 # expanded by the inner shell
    as_root sh -c 'cat > "$1"' sh "$DROPIN" <<EOF
# Written by the CineVault installer ($SITE/install.sh). Run it again to change this.
[Service]
User=$RUN_USER
Group=$run_group
LoadCredential=cinevault_key:$KEY
Environment=CINEVAULT_SECRET_KEY_FILE=%d/cinevault_key
Environment=CINEVAULT_WEB_PORT=$PORT
Environment="CINEVAULT_WEB_ROOTS=$roots"
$protect_home
EOF
    as_root install -d -m 0750 "$DATA"
    as_root chown -R "$RUN_USER:$run_group" "$DATA"
    printf '%s\n' "$MEDIA" | while IFS= read -r f; do
        if [ "$RUN_USER" = "$(id -un)" ]; then test -w "$f" && ok=1 || ok=""
        else as_root runuser -u "$RUN_USER" -- test -w "$f" 2>/dev/null && ok=1 || ok=""; fi
        [ -n "$ok" ] || say "Warning: $RUN_USER cannot write to $f. CineVault needs that to save artwork and NFO files and to rename."
    done
}

still_starting() {
    if [ "$MODE" = docker ]; then
        [ "$($DOCKER inspect -f '{{.State.Running}} {{.RestartCount}}' "$NAME" 2>/dev/null)" = "true 0" ]
    else
        as_root systemctl is-active --quiet "$UNIT" || [ "$(as_root systemctl is-active "$UNIT" 2>/dev/null)" = activating ]
    fi
}

n_logs() { as_root journalctl -u "$UNIT" -n 20 --no-pager; }

n_start() {
    if ! has_systemd; then
        say ""
        say "CineVault is installed, but this system does not run systemd, so it was not started."
        say "Start it with:"
        say "  sudo -u $RUN_USER env CINEVAULT_DATA_DIR=$DATA CINEVAULT_WEB_BIND=0.0.0.0 CINEVAULT_WEB_PORT=$PORT \\"
        say "    CINEVAULT_WEB_ROOTS='$(printf '%s\n' "$MEDIA" | paste -sd ':' -)' \\"
        say "    CINEVAULT_SECRET_KEY=\"\$(sudo cat $KEY)\" cinevault-server --ui /usr/share/cinevault/ui"
        exit 0
    fi
    step "Starting the cinevault-server service"
    as_root systemctl daemon-reload
    as_root systemctl enable --quiet "$UNIT"
    as_root systemctl restart "$UNIT"
    wait_url n_logs
}

n_finish() {
    lines=$(printf '%s\n' "$MEDIA" | sed 's/^/  /')
    finish "$1" "sudo systemctl restart $UNIT" "$lines"
    say "Logs: journalctl -u $UNIT   Stop: sudo systemctl stop $UNIT"
}

n_install() {
    say "CineVault browser mode (beta) installer"
    n_check_platform
    need_root
    ask_media
    pick_port
    n_download
    n_install_package
    n_configure
    n_start
    n_finish fresh
}

n_update() {
    n_check_platform
    need_root
    n_read_config
    n_download
    n_install_package
    n_configure
    n_start
    n_finish update
}

n_folders() {
    need_root
    n_read_config
    say "CineVault may now use these folders:"
    printf '%s\n' "$MEDIA" | sed 's/^/  /'
    ask_media
    n_configure
    n_start
    n_finish update
}

n_uninstall() {
    if ! n_installed; then say "The CineVault server is not installed."; return; fi
    yes_no "Remove the CineVault server? Its library and settings in $DATA are kept." y || exit 1
    need_root
    if has_systemd; then as_root systemctl disable --now "$UNIT" >/dev/null 2>&1 || true; fi
    if dpkg -s cinevault-server >/dev/null 2>&1; then as_root apt-get purge -y cinevault-server
    elif rpm -q cinevault-server >/dev/null 2>&1; then
        if command -v dnf >/dev/null 2>&1; then as_root dnf remove -y cinevault-server
        elif command -v yum >/dev/null 2>&1; then as_root yum remove -y cinevault-server
        else as_root zypper --non-interactive remove cinevault-server; fi
    else
        as_root rm -f /usr/local/bin/cinevault-server /usr/local/bin/cinevault /etc/systemd/system/cinevault-server.service
        as_root rm -rf /usr/share/cinevault
    fi
    as_root rm -rf "$DROPIN_DIR"
    # Removing the 1.2.0 .deb also removes an empty /usr/local/bin, which belongs to the system.
    as_root install -d -m 0755 /usr/local/bin
    if has_systemd; then as_root systemctl daemon-reload; fi
    say "CineVault is removed. Your movie folders were not touched."
    if [ -z "$YES" ] && [ "$(ask "Type DELETE to also erase its library, password and keys ($DATA, /etc/cinevault), or press Enter to keep them" "")" = DELETE ]; then
        as_root rm -rf "$DATA" /etc/cinevault
        say "Deleted $DATA and /etc/cinevault."
    else
        say "The library is still in $DATA; installing again picks it up."
    fi
}

# ==== Docker (macOS, and Linux with --docker) ========================================================
docker_ready() { $DOCKER info >/dev/null 2>&1; }

ensure_docker() {
    step "Checking Docker"
    if ! command -v docker >/dev/null 2>&1; then
        if [ "$OS" = Darwin ]; then
            say "Docker Desktop is not installed. Get it from https://docs.docker.com/desktop/setup/install/mac-install/"
            say "Open it once, accept its terms, then run this command again."
            exit 1
        fi
        say "Docker is not installed. Docker's official install script (https://get.docker.com) can set it up;"
        say "it needs your password for sudo. Instructions for other ways: https://docs.docker.com/engine/install/"
        yes_no "Install Docker now with that script?" n || exit 1
        curl -fsSL https://get.docker.com | sudo sh
        sudo usermod -aG docker "$(id -un)" || true
        DOCKER="sudo docker"
    fi
    if docker_ready; then return; fi
    if [ "$OS" = Darwin ]; then
        say "Starting Docker Desktop..."
        open -a Docker || die "Docker Desktop could not be started. Open it from Applications and run this again."
    elif docker info 2>&1 | grep -qi "permission denied"; then
        say "Your account may not use Docker yet. Using sudo for this run; to stop needing it, run"
        say "  sudo usermod -aG docker $(id -un)   and sign out and in again."
        DOCKER="sudo docker"
    else
        say "Docker is installed but not running. Trying: sudo systemctl start docker"
        sudo systemctl start docker || true
    fi
    i=0
    until docker_ready; do
        i=$((i + 1))
        [ "$i" -le 90 ] || die "Docker is not running. Start it (Docker Desktop, or: sudo systemctl start docker) and run this again."
        sleep 2
    done
}

d_exists() { command -v docker >/dev/null 2>&1 && $DOCKER container inspect "$NAME" >/dev/null 2>&1; }

d_read_existing() {
    BINDS=$($DOCKER inspect -f '{{range .HostConfig.Binds}}{{println .}}{{end}}' "$NAME")
    ENVS=$($DOCKER inspect -f '{{range .Config.Env}}{{println .}}{{end}}' "$NAME" |
        grep -E '^(PUID|PGID|TZ|CINEVAULT_SECRET_KEY|CINEVAULT_SECRET_KEY_FILE|CINEVAULT_WEB_TRUST_PROXY|CINEVAULT_WEB_ROOTS)=' || true)
    PORT=$($DOCKER inspect -f '{{(index (index .HostConfig.PortBindings "8787/tcp") 0).HostPort}}' "$NAME" 2>/dev/null || true)
    PORT="${PORT:-8787}"
}

d_mount_name() { # a short, safe name under /media for a host folder
    base=$(basename "$1" | tr -c 'A-Za-z0-9._-' '-' | sed 's/^-*//; s/-*$//')
    [ -n "$base" ] || base=media
    candidate=$base
    n=2
    while printf '%s\n' "$BINDS" | grep -q ":/media/$candidate\$"; do
        candidate="$base-$n"
        n=$((n + 1))
    done
    printf '%s' "$candidate"
}

d_media_binds() { # adds one /media/<name> bind per MEDIA folder to BINDS
    list=$MEDIA
    while IFS= read -r f; do
        [ -z "$f" ] || BINDS="$BINDS
$f:/media/$(d_mount_name "$f")"
    done <<EOF_BINDS
$list
EOF_BINDS
}

d_run() { # runs the container from BINDS, ENVS (newline-separated lists) and PORT
    set -- run -d --name "$NAME" -p "$PORT:8787" --restart unless-stopped --stop-timeout 30
    old_ifs=$IFS
    IFS='
'
    for b in $BINDS; do set -- "$@" -v "$b"; done
    for e in $ENVS; do set -- "$@" -e "$e"; done
    IFS=$old_ifs
    $DOCKER "$@" "$IMAGE" >/dev/null
}

d_pull() {
    step "Downloading $IMAGE"
    $DOCKER pull "$IMAGE" >/dev/null || die "The image could not be downloaded. Check the internet connection and try again."
}

d_logs() { $DOCKER logs --tail 20 "$NAME"; }

d_finish() {
    lines=$(printf '%s\n' "$BINDS" | sed -n 's#^\(.*\):\(/media/[^:]*\).*$#  \2    (your folder \1)#p')
    finish "$1" "docker restart cinevault" "$lines"
}

d_not_compose() {
    if [ -n "$($DOCKER inspect -f '{{index .Config.Labels "com.docker.compose.project"}}' "$NAME")" ]; then
        die "This CineVault was started with Docker Compose. Change it in its docker-compose.yml, and update it from that folder: docker compose pull && docker compose up -d"
    fi
}

d_recreate() {
    step "Replacing the container (your data folder is kept)"
    $DOCKER stop "$NAME" >/dev/null 2>&1 || true
    $DOCKER rm "$NAME" >/dev/null
    d_run
    wait_url d_logs
    d_finish update
}

d_update() {
    d_not_compose
    d_read_existing
    d_pull
    d_recreate
}

d_folders() {
    d_not_compose
    d_read_existing
    say "CineVault now sees these folders:"
    printf '%s\n' "$BINDS" | grep ':/media/' | sed 's/^/  /' || true
    BINDS=$(printf '%s\n' "$BINDS" | grep -v ':/media/' || true)
    ask_media
    d_media_binds
    d_recreate
}

d_uninstall() {
    if ! d_exists; then say "CineVault is not installed (no container named $NAME)."; return; fi
    d_read_existing
    data=$(printf '%s\n' "$BINDS" | grep ':/data$' | sed 's#:/data$##' || true)
    yes_no "Remove the CineVault container? Your data folder ($data) is kept." y || exit 1
    $DOCKER stop "$NAME" >/dev/null 2>&1 || true
    $DOCKER rm "$NAME" >/dev/null
    $DOCKER image rm "$IMAGE" >/dev/null 2>&1 || true
    say "CineVault is removed. Your movie folders were not touched."
    if [ -n "$data" ] && [ -z "$YES" ] && [ "$(ask "Type DELETE to also erase the data folder (library, password, keys), or press Enter to keep it" "")" = DELETE ]; then
        rm -rf "$data"
        key=$(printf '%s\n' "$BINDS" | grep ':/run/secrets/cinevault_key' | sed 's#:/run/secrets/cinevault_key.*##' || true)
        [ -z "$key" ] || [ ! -f "$key" ] || rm -f "$key"
        rmdir "$(dirname "$data")" 2>/dev/null || true
        say "Deleted $data and its key."
    elif [ -n "$data" ]; then
        say "The data folder is still at $data; running the installer again picks it up."
    fi
}

d_install() {
    say "CineVault browser mode (beta) installer, with Docker"
    ask_media
    step "Where CineVault keeps its own data"
    dir=$(ask "Data folder (library, settings, password)" "${CINEVAULT_DIR:-$HOME/cinevault}")
    case "$dir" in "~"*) dir="$HOME${dir#\~}" ;; esac
    mkdir -p "$dir/data"
    dir=$(cd "$dir" && pwd -P)
    key="$dir/secret.key"
    if [ ! -s "$key" ]; then
        # The key that encrypts your TMDB and other provider keys. Keep it with the data folder.
        (umask 077; openssl rand -base64 32 >"$key" 2>/dev/null || head -c 32 /dev/urandom | base64 >"$key")
    fi
    BINDS="$dir/data:/data
$key:/run/secrets/cinevault_key:ro"
    d_media_binds

    uid=$(id -u); gid=$(id -g)
    if [ "$uid" -eq 0 ]; then uid="${SUDO_UID:-1000}"; gid="${SUDO_GID:-1000}"; fi
    ENVS="PUID=$uid
PGID=$gid
CINEVAULT_SECRET_KEY_FILE=/run/secrets/cinevault_key"
    pick_port
    d_pull
    step "Starting CineVault"
    d_run
    wait_url d_logs
    d_finish fresh
}

# ==== what to do =====================================================================================
main() {
    # An existing Docker install on Linux is kept in Docker.
    if [ "$MODE" = native ] && ! n_installed && d_exists; then MODE=docker; fi

    if [ "$MODE" = docker ]; then
        ensure_docker
        installed=d_exists
    else
        installed=n_installed
    fi
    case "$ACTION" in
        install) if $installed; then menu; fi ;;
        update | folders) $installed || die "CineVault is not installed yet. Run the installer without '$ACTION'." ;;
        uninstall | remove) ACTION=uninstall ;;
        *) die "Unknown action '$ACTION'. Use install, update, folders or uninstall (and --docker for Docker)." ;;
    esac
    if [ "$MODE" = docker ]; then prefix=d_; else prefix=n_; fi
    "$prefix$ACTION"
}

# The whole script is read before anything runs, and no command it starts can read the rest of the
# script from standard input (questions are read from the terminal).
main </dev/null
