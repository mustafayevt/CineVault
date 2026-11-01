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
# A library folder can be on a network share: give //nas/movies, \\nas\movies or smb://nas/movies,
# with a folder inside it if you like (//nas/movies/Films). The installer asks for the share's user
# and password (the password is not shown) and never puts the password on a command line. On
# Linux it mounts the share at /mnt/<server>-<share> with a systemd mount unit and keeps the login
# in /etc/cinevault/shares, readable only by root; with Docker on Linux the container uses that
# mount. With Docker Desktop on macOS it makes a CIFS volume, cinevault-<server>-<share>, which
# keeps the login in Docker's volume options.
#
# Without questions (all optional except CINEVAULT_MEDIA on a first install):
#   CINEVAULT_YES=1                         accept every default, never ask
#   CINEVAULT_MEDIA="/srv/movies;/srv/tv"   library folders, separated by ";" (shares too: //nas/movies)
#   CINEVAULT_SHARE_USER=alex               the user for every share in CINEVAULT_MEDIA
#   CINEVAULT_SHARE_PASSWORD_FILE=/root/nas-password   a file whose first line is that user's password
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
SELF_TEST=""
for arg in "$@"; do
    case "$arg" in
        --docker) MODE=docker ;;
        --self-test) SELF_TEST=1 ;;
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
if [ -z "$SELF_TEST" ]; then
    case "$OS" in
        Linux) ;;
        Darwin) MODE=docker ;;
        *) die "This script is for Linux and macOS. On Windows, open PowerShell and run: irm $SITE/install.ps1 | iex" ;;
    esac
    command -v curl >/dev/null 2>&1 || die "curl is needed. Install it with your package manager and run this again."
fi

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

media_line() { # adds one line to MEDIA, once
    if printf '%s\n' "$MEDIA" | grep -qxF -e "$1"; then return; fi
    MEDIA="${MEDIA:+$MEDIA
}$1"
}

add_media() { # checks a folder (or a network share) and adds its full path to MEDIA
    folder=$(printf '%s' "$1" | sed "s/^[\"']//; s/[\"']\$//; s#/*\$##")
    if is_share "$folder"; then add_share "$folder"; return; fi
    case "$folder" in "~"*) folder="$HOME${folder#\~}" ;; esac
    [ -n "$folder" ] || folder=/
    if [ ! -d "$folder" ]; then say "  There is no folder $folder."; return 1; fi
    folder=$(cd "$folder" && pwd -P)
    media_line "$folder"
}

ask_media() { # fills MEDIA with the library folders
    MEDIA=""
    SHARES=""
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
        say "A folder on a network share works too: type it like //nas/movies or smb://nas/movies/Films."
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

# ---- network shares -----------------------------------------------------------------------------
# A share's password is read from the terminal without showing it (or from the first line of
# CINEVAULT_SHARE_PASSWORD_FILE), kept in a shell variable and written only to the standard input
# of the program that stores it, so it never appears in a command line or in this script's output.
SHARE_DIR=/etc/cinevault/shares
SHARE_MARK="# Written by the CineVault installer"
SHARES=""   # Linux: the shares checked in this run, one "key|server|share|SMB version" per line
SH_USER=""
SH_PW=""

is_share() { case "$1" in //* | \\\\* | [Ss][Mm][Bb]://*) return 0 ;; *) return 1 ;; esac; }

share_key() { # share_key server share -> a name for files, folders and volumes: nas-movies
    printf '%s-%s' "$1" "$2" | tr '[:upper:]' '[:lower:]' | tr -c 'a-z0-9._-' '-' | sed 's/-*$//'
}

parse_share() { # parse_share address -> S_SERVER, S_SHARE, S_SUB (a folder inside it or ""), S_KEY, S_HINT
    a=$(printf '%s' "$1" | tr '\134' '/')   # \134 is a backslash
    case "$a" in [Ss][Mm][Bb]://*) a=${a#*://} ;; *) a=${a#//} ;; esac
    a=$(printf '%s' "$a" | sed 's#//*#/#g; s#/$##')
    S_SERVER=${a%%/*}
    S_HINT=""   # the user in smb://alex@nas/movies
    case "$S_SERVER" in *@*) S_HINT=${S_SERVER%@*}; S_SERVER=${S_SERVER##*@} ;; esac
    rest=${a#*/}
    [ "$rest" != "$a" ] || rest=""
    S_SHARE=${rest%%/*}
    S_SUB=${rest#*/}
    [ "$S_SUB" != "$rest" ] || S_SUB=""
    if ! printf '%s' "$S_SERVER" | grep -Eq '^[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?$'; then
        say "  $1 does not start with a server name or IPv4 address, like //nas/movies."
        return 1
    fi
    if [ -z "$S_SHARE" ]; then say "  $1 names no share. Type it like //$S_SERVER/movies."; return 1; fi
    if ! printf '%s' "$S_SHARE" | grep -Eq '^[A-Za-z0-9._$ -]{1,80}$'; then
        say "  The share name \"$S_SHARE\" has a sign the installer cannot use (letters, digits, spaces, . _ - and \$ are fine)."
        return 1
    fi
    case "/$S_SUB/" in */../* | */./*) say "  The folder inside the share may not have . or .. in its path."; return 1 ;; esac
    if printf '%s' "$S_SUB" | grep -q '[[:cntrl:]]'; then say "  The folder inside the share has a control character."; return 1; fi
    S_KEY=$(share_key "$S_SERVER" "$S_SHARE")
}

unit_name() { # unit_name key -> the systemd mount unit for /mnt/<key>, as systemd-escape --path names it
    printf 'mnt-%s.mount' "$(printf '%s' "$1" | sed 's/-/\\x2d/g')"
}

unit_text() { # unit_text key server share version uid gid -> the mount unit
    printf '%s\n' "$SHARE_MARK ($SITE/install.sh). Run it again to change or remove this." \
        "[Unit]" \
        "Description=CineVault network share //$2/$3" \
        "Wants=network-online.target" \
        "After=network-online.target" \
        "" \
        "[Mount]" \
        "What=//$2/$3" \
        "Where=/mnt/$1" \
        "Type=cifs" \
        "Options=credentials=$SHARE_DIR/$1.cred,uid=$5,gid=$6,file_mode=0664,dir_mode=0775,iocharset=utf8,vers=$4,_netdev,nofail" \
        "TimeoutSec=30" \
        "" \
        "[Install]" \
        "WantedBy=remote-fs.target"
}

cred_text() { # cred_text user password -> a mount.cifs credentials file (DOMAIN\user gives domain=)
    case "$1" in
        *\\*) printf 'username=%s\npassword=%s\ndomain=%s\n' "${1#*\\}" "$2" "${1%%\\*}" ;;
        *) printf 'username=%s\npassword=%s\n' "$1" "$2" ;;
    esac
}

json_str() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

vol_body() { # vol_body name server share user password version uid gid -> Docker's body for a CIFS volume
    v_user=$4
    v_dom=""
    case "$v_user" in *\\*) v_dom=",domain=${v_user%%\\*}"; v_user=${v_user#*\\} ;; esac
    v_pw=$(printf '%s' "$5" | sed 's/,/,,/g')   # a comma in a mount option value is written twice
    v_o="addr=$2,username=$v_user$v_dom,password=$v_pw,uid=$7,gid=$8,file_mode=0664,dir_mode=0775,vers=$6"
    printf '{"Name":"%s","Driver":"local","DriverOpts":{"type":"cifs","device":"%s","o":"%s"},"Labels":{"cinevault.share":"%s","cinevault.key":"%s"}}\n' \
        "$1" "$(json_str "//$2/$3")" "$(json_str "$v_o")" "$(json_str "//$2/$3")" "$(share_key "$2" "$3")"
}

share_mask() { sed 's/password=.*,uid=/password=********,uid=/'; }

share_kind() { # share_kind "mount output" -> login, missing, noname or other
    case "$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')" in
        *"error(13)"* | *"permission denied"* | *"logon failure"*) echo login ;;
        *"error(2)"* | *"no such file"*) echo missing ;;
        *resolve*) echo noname ;;
        *) echo other ;;
    esac
}

share_error() { # share_error "mount output" -> why //S_SERVER/S_SHARE could not be mounted, in plain words
    case "$(share_kind "$1")" in
        login) echo "Wrong user name or password for //$S_SERVER/$S_SHARE." ;;
        missing) echo "There is no share named $S_SHARE on $S_SERVER." ;;
        noname) echo "$S_SERVER could not be reached. Check the name or address and that it is switched on." ;;
        *)
            case "$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')" in
                *"error(112)"* | *"error(113)"* | *"error(110)"* | *"error(111)"* | *"host is down"* | *"no route"* | *"timed out"* | *unreachable*)
                    echo "$S_SERVER could not be reached. Check the name or address and that it is switched on." ;;
                *) echo "//$S_SERVER/$S_SHARE could not be mounted: $(printf '%s\n' "$1" | grep -i error | head -n 1 | share_mask)" ;;
            esac ;;
    esac
}

ask_secret() { # ask_secret "Question" -> SH_PW, read from the terminal without showing it
    has_tty || die "No terminal to ask for the password on. Set CINEVAULT_SHARE_USER and CINEVAULT_SHARE_PASSWORD_FILE."
    printf '%s: ' "$1" >/dev/tty
    tty_saved=$(stty -g </dev/tty)
    trap 'stty "$tty_saved" </dev/tty; printf "\n" >/dev/tty; exit 130' INT TERM
    stty -echo </dev/tty
    SH_PW=""
    IFS= read -r SH_PW </dev/tty || true
    stty "$tty_saved" </dev/tty
    trap - INT TERM
    printf '\n' >/dev/tty
}

ask_login() { # SH_USER and SH_PW for //S_SERVER/S_SHARE
    if [ -n "$YES" ] || [ -n "${CINEVAULT_SHARE_PASSWORD_FILE:-}" ]; then
        SH_USER=${CINEVAULT_SHARE_USER:-$S_HINT}
        pw_file=${CINEVAULT_SHARE_PASSWORD_FILE:-}
        if [ -z "$SH_USER" ] || [ -z "$pw_file" ]; then
            die "For the share //$S_SERVER/$S_SHARE, set CINEVAULT_SHARE_USER and CINEVAULT_SHARE_PASSWORD_FILE (a file whose first line is the password)."
        fi
        [ -r "$pw_file" ] || die "The password file $pw_file cannot be read."
        SH_PW=""
        IFS= read -r SH_PW <"$pw_file" || true
    else
        prev_user=$SH_USER   # the user typed for the share before is offered again
        SH_USER=""
        while [ -z "$SH_USER" ]; do
            SH_USER=$(ask "User for //$S_SERVER/$S_SHARE" "${CINEVAULT_SHARE_USER:-${S_HINT:-$prev_user}}")
        done
        ask_secret "Password for $SH_USER on $S_SERVER (it is not shown)"
    fi
    case "$SH_USER" in *,* | *=*) die "A user name with , or = cannot be used for a share." ;; esac
    if printf '%s' "$SH_USER" | grep -q '[[:cntrl:]]' || printf '%s' "$SH_PW" | grep -q '[[:cntrl:]]'; then
        die "The user name or password for //$S_SERVER/$S_SHARE has a control character."
    fi
}

add_share() { # a share address: sets the share up (or reuses it) and adds its folder to MEDIA
    parse_share "$1" || return 1
    if [ "$OS" = Darwin ]; then d_share_volume; else host_share; fi
}

# Linux (the service, and Docker on Linux): the share is mounted by a systemd mount unit at
# /mnt/<server>-<share>, with its login in a credentials file only root can read.
need_cifs() {
    if command -v mount.cifs >/dev/null 2>&1 || [ -x /sbin/mount.cifs ] || [ -x /usr/sbin/mount.cifs ]; then return; fi
    yes_no "Network shares need the cifs-utils package. Install it now?" y || die "Install cifs-utils with your package manager and run this again."
    step "Installing cifs-utils"
    case "$(n_kind)" in
        deb)
            as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends cifs-utils </dev/null || {
                as_root apt-get update </dev/null &&
                    as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends cifs-utils </dev/null
            } ;;
        rpm)
            if command -v dnf >/dev/null 2>&1; then as_root dnf install -y cifs-utils </dev/null
            elif command -v yum >/dev/null 2>&1; then as_root yum install -y cifs-utils </dev/null
            else as_root zypper --non-interactive install cifs-utils </dev/null; fi ;;
        *) false ;;
    esac || die "cifs-utils could not be installed. Install it with your package manager and run this again."
}

share_write_cred() { # writes SH_USER and SH_PW to <key>.new, a file only root can read from the start
    as_root install -d -m 0700 "$SHARE_DIR" || die "$SHARE_DIR could not be made."
    # shellcheck disable=SC2016 # expanded by the inner shell
    cred_text "$SH_USER" "$SH_PW" | as_root sh -c 'umask 077; cat > "$1"' sh "$SHARE_DIR/$S_KEY.new" ||
        die "The login for //$S_SERVER/$S_SHARE could not be saved."
}

share_mount_check() { # asks the login and mounts the share once with it, to check it and the folder
    mnt=/mnt/$S_KEY
    checked=$(printf '%s\n' "$SHARES" | awk -F '|' -v k="$S_KEY" '$1 == k { print $4 }')
    if [ -n "$checked" ]; then
        # A second folder on a share checked a moment ago: its login is saved already.
        as_root mount -t cifs "//$S_SERVER/$S_SHARE" "$mnt" -o "credentials=$SHARE_DIR/$S_KEY.cred,vers=$checked" ||
            die "//$S_SERVER/$S_SHARE could not be mounted again."
        found=1
        if [ -n "$S_SUB" ] && ! as_root test -d "$mnt/$S_SUB"; then say "  $S_SUB was not found on //$S_SERVER/$S_SHARE."; found=""; fi
        as_root umount "$mnt" || die "$mnt could not be unmounted after the check."
        [ -n "$found" ]
        return
    fi
    tries=0
    while :; do
        ask_login
        share_write_cred
        SH_PW=""
        as_root install -d -m 0755 "$mnt" || die "$mnt could not be made."
        kind=other
        # SMB 3.0 first; some older NAS units only speak 2.1.
        for vers in 3.0 2.1; do
            if out=$(as_root mount -t cifs "//$S_SERVER/$S_SHARE" "$mnt" -o "credentials=$SHARE_DIR/$S_KEY.new,vers=$vers" 2>&1); then
                kind=ok
                break
            fi
            kind=$(share_kind "$out")
            [ "$kind" = other ] || break
        done
        [ "$kind" != ok ] || break
        say "  $(share_error "$out")"
        tries=$((tries + 1))
        if [ "$kind" != login ] || [ -n "$YES" ] || [ -n "${CINEVAULT_SHARE_PASSWORD_FILE:-}" ] || [ "$tries" -ge 3 ]; then
            share_drop_check
            return 1
        fi
    done
    found=1
    if [ -n "$S_SUB" ] && ! as_root test -d "$mnt/$S_SUB"; then say "  $S_SUB was not found on //$S_SERVER/$S_SHARE."; found=""; fi
    as_root umount "$mnt" || die "$mnt could not be unmounted after the check."
    if [ -z "$found" ]; then share_drop_check; return 1; fi
    as_root mv -f "$SHARE_DIR/$S_KEY.new" "$SHARE_DIR/$S_KEY.cred" || die "The login for //$S_SERVER/$S_SHARE could not be saved."
    SHARES="${SHARES:+$SHARES
}$S_KEY|$S_SERVER|$S_SHARE|$vers"
}

share_drop_check() { # removes what a failed check left: the new login, and the mount folder of a new share
    as_root rm -f "$SHARE_DIR/$S_KEY.new"
    if [ ! -f "/etc/systemd/system/$(unit_name "$S_KEY")" ] && ! mountpoint -q "/mnt/$S_KEY"; then
        as_root rmdir "/mnt/$S_KEY" 2>/dev/null || true
    fi
}

host_share() { # mounts //S_SERVER/S_SHARE (or reuses its mount) and adds its folder to MEDIA
    has_systemd || die "Network shares are mounted at start-up by systemd, which this system does not run. Mount //$S_SERVER/$S_SHARE yourself and give its folder instead."
    need_root
    need_cifs
    mnt=/mnt/$S_KEY
    unit_file=/etc/systemd/system/$(unit_name "$S_KEY")
    if [ -f "$unit_file" ] && ! mountpoint -q "$mnt"; then as_root systemctl start "$(unit_name "$S_KEY")" >/dev/null 2>&1 || true; fi
    if mountpoint -q "$mnt"; then
        if ! grep -q "^$SHARE_MARK" "$unit_file" 2>/dev/null; then
            say "  Something else is mounted at $mnt. Give that folder instead of the share address."
            return 1
        fi
        if [ -n "$S_SUB" ] && ! as_root test -d "$mnt/$S_SUB"; then say "  $S_SUB was not found on //$S_SERVER/$S_SHARE."; return 1; fi
        say "  //$S_SERVER/$S_SHARE is mounted at $mnt already, with the login saved before."
    else
        share_mount_check || return 1
    fi
    if [ "$MODE" = docker ]; then media_line "$mnt"; else media_line "$mnt${S_SUB:+/$S_SUB}"; fi
}

shares_apply() { # shares_apply uid gid: writes and starts the mount units of the shares checked in this run
    [ -n "$SHARES" ] || return 0
    step "Mounting the network shares"
    list=$SHARES
    while IFS='|' read -r key server share vers; do
        # shellcheck disable=SC2016 # expanded by the inner shell
        unit_text "$key" "$server" "$share" "$vers" "$1" "$2" |
            as_root sh -c 'umask 022; cat > "$1.new" && mv -f "$1.new" "$1"' sh "/etc/systemd/system/$(unit_name "$key")"
    done <<EOF_UNITS
$list
EOF_UNITS
    as_root systemctl daemon-reload
    while IFS='|' read -r key server share vers; do
        as_root systemctl enable --quiet "$(unit_name "$key")"
        as_root systemctl start "$(unit_name "$key")" || true
        if ! mountpoint -q "/mnt/$key"; then
            as_root journalctl -u "$(unit_name "$key")" -n 10 --no-pager >&2 || true
            die "//$server/$share could not be mounted at /mnt/$key. The lines above say why."
        fi
        say "//$server/$share is mounted at /mnt/$key (SMB $vers)."
    done <<EOF_UNITS
$list
EOF_UNITS
}

share_units_of() { # share_units_of "folders" -> the installer's mount units they are on, separated by spaces
    printf '%s\n' "$1" | while IFS= read -r f; do
        case "$f" in /mnt/?*) ;; *) continue ;; esac
        key=${f#/mnt/}
        key=${key%%/*}
        if grep -q "^$SHARE_MARK" "/etc/systemd/system/$(unit_name "$key")" 2>/dev/null; then unit_name "$key"; echo; fi
    done | sort -u | paste -sd ' ' -
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
    if [ "$(id -u)" -ne 0 ] && [ -z "$SUDO" ]; then
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
    shares_apply "$(id -u "$RUN_USER")" "$(id -g "$RUN_USER")"
    # The service only wants the share mounts: a NAS that is off never keeps CineVault from starting,
    # and its folders read offline until the share is mounted again.
    share_units=$(share_units_of "$MEDIA")
    unit_section=""
    [ -z "$share_units" ] || unit_section="[Unit]
Wants=$share_units
After=$share_units
"
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
${unit_section}[Service]
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
    for m in $(printf '%s\n' "$MEDIA" | sed -n 's#^\(/mnt/[^/]*\).*#\1#p' | sort -u); do
        if [ -n "$(share_units_of "$m")" ]; then
            say "Network share at $m: mounted at start-up. If the NAS was off, mount it again with: sudo systemctl start $m"
        fi
    done
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

# ==== self-test (sh install.sh --self-test): the share parsing and the files the installer writes ===
self_test() {
    fails=0
    expect() { # expect "what" "got" "expected"
        if [ "$2" != "$3" ]; then
            printf 'FAIL %s\n  expected: %s\n  got:      %s\n' "$1" "$3" "$2"
            fails=$((fails + 1))
        fi
    }
    parsed() { if parse_share "$1" >/dev/null; then printf '%s|%s|%s|%s|%s' "$S_SERVER" "$S_SHARE" "$S_SUB" "$S_KEY" "$S_HINT"; else printf refused; fi; }
    shared() { if is_share "$1"; then printf yes; else printf no; fi; }

    expect "share //nas/movies" "$(shared //nas/movies)" yes
    expect "share \\\\nas\\movies" "$(shared '\\nas\movies')" yes
    expect "share smb://nas/movies" "$(shared smb://nas/movies)" yes
    expect "share SMB://nas/movies" "$(shared SMB://nas/movies)" yes
    expect "local /srv/movies" "$(shared /srv/movies)" no
    expect "local movies/Films" "$(shared movies/Films)" no

    expect "parse //nas/movies" "$(parsed //nas/movies)" "nas|movies||nas-movies|"
    expect "parse //nas/movies/Films" "$(parsed //nas/movies/Films)" "nas|movies|Films|nas-movies|"
    expect "parse \\\\NAS\\Movies\\Films\\HD" "$(parsed '\\NAS\Movies\Films\HD')" "NAS|Movies|Films/HD|nas-movies|"
    expect "parse smb://alex@nas.local/movies" "$(parsed smb://alex@nas.local/movies)" "nas.local|movies||nas.local-movies|alex"
    expect "parse smb://192.168.1.5/Media Library/TV" "$(parsed 'smb://192.168.1.5/Media Library/TV')" "192.168.1.5|Media Library|TV|192.168.1.5-media-library|"
    expect "parse //nas/movies\$" "$(parsed '//nas/movies$')" "nas|movies\$||nas-movies|"
    expect "parse //nas//movies//Films" "$(parsed //nas//movies//Films)" "nas|movies|Films|nas-movies|"
    expect "refuse //nas" "$(parsed //nas)" refused
    expect "refuse //-nas/movies" "$(parsed //-nas/movies)" refused
    expect "refuse //[::1]/movies" "$(parsed '//[::1]/movies')" refused
    expect "refuse //nas/mo*vies" "$(parsed '//nas/mo*vies')" refused
    expect "refuse //nas/mo%vies" "$(parsed '//nas/mo%vies')" refused
    expect "refuse //nas/movies/../etc" "$(parsed //nas/movies/../etc)" refused
    expect "refuse //nas/movies/./x" "$(parsed //nas/movies/./x)" refused

    expect "unit name" "$(unit_name nas-movies)" 'mnt-nas\x2dmovies.mount'
    expect "unit name with dots" "$(unit_name 192.168.1.5-media-library)" 'mnt-192.168.1.5\x2dmedia\x2dlibrary.mount'
    if command -v systemd-escape >/dev/null 2>&1; then
        expect "unit name as systemd names it" "$(unit_name 192.168.1.5-media-library)" \
            "$(systemd-escape --path --suffix=mount /mnt/192.168.1.5-media-library)"
    fi

    expect "credentials" "$(cred_text alex 'pa ss,w"rd\x')" "username=alex
password=pa ss,w\"rd\\x"
    expect "credentials with a domain" "$(cred_text 'WORK\alex' secret)" "username=alex
password=secret
domain=WORK"

    unit=$(unit_text nas-movies nas movies 2.1 1000 1001)
    expect "unit marker" "$(printf '%s\n' "$unit" | head -n 1 | grep -c "^$SHARE_MARK")" 1
    expect "unit What" "$(printf '%s\n' "$unit" | sed -n 's/^What=//p')" //nas/movies
    expect "unit Where" "$(printf '%s\n' "$unit" | sed -n 's/^Where=//p')" /mnt/nas-movies
    expect "unit Options" "$(printf '%s\n' "$unit" | sed -n 's/^Options=//p')" \
        "credentials=/etc/cinevault/shares/nas-movies.cred,uid=1000,gid=1001,file_mode=0664,dir_mode=0775,iocharset=utf8,vers=2.1,_netdev,nofail"
    expect "unit WantedBy" "$(printf '%s\n' "$unit" | sed -n 's/^WantedBy=//p')" remote-fs.target
    expect "no password in the unit" "$(printf '%s\n' "$unit" | grep -c password)" 0

    S_SERVER=nas S_SHARE=movies
    expect "error wrong login" "$(share_error 'mount error(13): Permission denied')" "Wrong user name or password for //nas/movies."
    expect "error no share" "$(share_error 'mount error(2): No such file or directory')" "There is no share named movies on nas."
    expect "error no server" "$(share_error 'mount error: could not resolve address for nas: Unknown error')" \
        "nas could not be reached. Check the name or address and that it is switched on."
    expect "error host down" "$(share_error 'mount error(112): Host is down')" \
        "nas could not be reached. Check the name or address and that it is switched on."
    expect "error other" "$(share_error 'mount error(95): Operation not supported')" \
        "//nas/movies could not be mounted: mount error(95): Operation not supported"
    expect "retry with SMB 2.1 after" "$(share_kind 'mount error(95): Operation not supported')" other
    expect "no retry after a wrong login" "$(share_kind 'mount error(13): Permission denied')" login

    if [ "$fails" -eq 0 ]; then echo "install.sh self-test passed"; else echo "install.sh self-test: $fails failed"; exit 1; fi
}
if [ -n "$SELF_TEST" ]; then self_test; exit 0; fi

# The whole script is read before anything runs, and no command it starts can read the rest of the
# script from standard input (questions are read from the terminal).
main </dev/null
