#!/bin/sh
# Container entrypoint for cinevault-server.
#
# Runs as root only long enough to map the cinevault user to PUID/PGID and make /data writable,
# then replaces itself with the server as that unprivileged user (no root process is left behind).
set -eu

PUID="${PUID:-1000}"
PGID="${PGID:-1000}"

case "$PUID" in
    ''|*[!0-9]*) echo "entrypoint: PUID must be a number, got '$PUID'" >&2; exit 64 ;;
esac
case "$PGID" in
    ''|*[!0-9]*) echo "entrypoint: PGID must be a number, got '$PGID'" >&2; exit 64 ;;
esac
if [ "$PUID" -eq 0 ] || [ "$PGID" -eq 0 ]; then
    echo "entrypoint: PUID and PGID must not be 0; the server does not run as root" >&2
    exit 64
fi

# The interface is fixed by the image; the address and port can be overridden from the environment.
# They are passed as flags as well because older server builds do not read the environment.
set -- /usr/local/bin/cinevault-server --ui /usr/share/cinevault/ui
if [ -n "${CINEVAULT_WEB_BIND:-}" ]; then
    set -- "$@" --bind "$CINEVAULT_WEB_BIND"
fi
if [ -n "${CINEVAULT_WEB_PORT:-}" ]; then
    set -- "$@" --port "$CINEVAULT_WEB_PORT"
fi

if [ "$(id -u)" -eq 0 ]; then
    if [ "$PGID" != "$(id -g cinevault)" ]; then
        groupmod -o -g "$PGID" cinevault
    fi
    if [ "$PUID" != "$(id -u cinevault)" ]; then
        usermod -o -u "$PUID" cinevault
    fi

    # The data volume belongs to the mapped user. The media volumes are left alone: their files
    # may live on a NAS share with ownership that must not be rewritten on every start.
    chown -R "$PUID:$PGID" /data
    export HOME=/data

    exec setpriv --reuid "$PUID" --regid "$PGID" --clear-groups "$@"
fi

# Started with `--user` already. PUID/PGID cannot be applied without root, so refuse a mismatch
# instead of running as an unexpected account.
if [ "$(id -u)" != "$PUID" ] || [ "$(id -g)" != "$PGID" ]; then
    echo "entrypoint: running as $(id -u):$(id -g), but PUID:PGID is $PUID:$PGID; start as root to remap or set matching PUID/PGID" >&2
    exit 64
fi

exec "$@"
