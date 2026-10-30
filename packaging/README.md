# CineVault server packaging

CineVault runs next to the media files and serves the same interface to a browser on another
machine. This folder and the scripts beside it hold the three ways to install that server:

| Install | Artifact | Best for |
| --- | --- | --- |
| Container | `ghcr.io/mustafayevt/cinevault` | A NAS or any host with Docker |
| Packages | `cinevault-server_<version>_<arch>.deb` / `.rpm` | A normal Linux server |
| Tarball | `cinevault-server-<version>-linux-<arch>.tar.gz` | A host with no package manager |

The desktop app is still named `cinevault`; the server packages are `cinevault-server`, so both can
be installed on one machine.

## Install paths

The packages and the tarball/container carry the same files:

| Path | Contents |
| --- | --- |
| `/usr/local/bin/cinevault-server` | The browser server |
| `/usr/local/bin/cinevault` | The command line |
| `/usr/share/cinevault/ui` | The built interface |
| `/usr/lib/systemd/system/cinevault-server.service` | The systemd unit example |

The package creates the `cinevault` system user and `/srv/cinevault`. The tarball keeps everything
in its own folder and `run.sh` uses `<folder>/data`; the container uses `/data`.

## Container

```sh
docker build -t cinevault-server .          # or: bun run server:image
docker run -d --name cinevault \
  -p 8787:8787 \
  -e PUID=1000 -e PGID=1000 \
  -v "$PWD/data:/data" \
  -v /srv/media:/media \
  --restart unless-stopped \
  cinevault-server
```

Or edit `docker-compose.yml` and run `docker compose up -d`. The image is published as
`ghcr.io/mustafayevt/cinevault:<version>` and `:latest`.

- **Volumes:** `/data` holds the library database, settings, sessions, downloaded tools and the
  provider-key file. Keep it on a local filesystem, not an SMB or NFS share. `/media` is where the
  library folders live; mount each one read-write.
- **Permissions:** the entrypoint maps the image's `cinevault` user to `PUID`/`PGID` (both default
  1000), makes `/data` belong to it and then runs the server as that user. Library files must be
  readable and writable by the same UID/GID. If the media belongs to `1001:1001`, start the
  container with `PUID=1001 PGID=1001`.
- **No root process is left behind:** the entrypoint uses `setpriv` to replace itself with the
  unprivileged server. Starting with `--user` works too, but then `PUID`/`PGID` must match it.
- **Library folders are read-write:** Scan, Rename, artwork and NFO writes all change files beside
  the media. A read-only mount lets the library open, but setup refuses the folder with "This
  folder is not writable by the server."
- **Health:** the image runs `curl -fsS http://127.0.0.1:8787/healthz` every 30 seconds. Docker
  marks the container unhealthy after three failures.
- **Stop:** the image sends `SIGINT` on stop, the signal the server uses for a clean shutdown.

The server listens on `0.0.0.0:8787` inside the container. Publish the port only on the network
you trust; for HTTPS put a reverse proxy in front and set `CINEVAULT_WEB_TRUST_PROXY` to the proxy
address (see the main documentation).

### Provider keys

The master key for the encrypted provider-key file is supplied as a Docker secret, never baked
into an image layer or written to `settings.json`:

```yaml
services:
  cinevault:
    environment:
      CINEVAULT_SECRET_KEY_FILE: /run/secrets/cinevault_key
    secrets:
      - cinevault_key

secrets:
  cinevault_key:
    file: ./secrets/cinevault_key
```

Create `./secrets/cinevault_key` with `openssl rand -base64 32 > secrets/cinevault_key` (the key must be base64 of exactly 32 bytes; any other value stops the server at start) and keep it out of version control (the
repository's `.gitignore` and `.dockerignore` already exclude `secrets/`). Without a key the server
still opens the library; Settings says provider keys are locked.

## Debian and RPM packages

On a Linux build host with Rust, bun, `dpkg-deb` and `rpmbuild`:

```sh
bun run server:package
```

The artifacts land in `target/server-package/`:

```text
cinevault-server_<version>_amd64.deb
cinevault-server-<version>-1.x86_64.rpm
```

Install one:

```sh
sudo apt install ./cinevault-server_<version>_amd64.deb
# or
sudo dnf install ./cinevault-server-<version>-1.x86_64.rpm
```

The package creates the `cinevault` user and `/srv/cinevault`, but it does not start the service.
Do that once:

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now cinevault-server
```

The systemd unit sets `CINEVAULT_DATA_DIR=/srv/cinevault`, binds the server to the network and
restarts it on failure. If a library folder lives under `/home`, remove `ProtectHome=true` from the
unit; if it lives outside `/srv` and `/media`, adjust `ProtectSystem` as well.

### Master key with systemd

Instead of an environment variable, use a systemd credential (mode `0600`, owned by root):

```sh
sudo install -d -m 0755 /etc/cinevault
sudo sh -c 'head -c 48 /dev/urandom | base64 > /etc/cinevault/cinevault.key'
sudo chmod 0600 /etc/cinevault/cinevault.key
```

Uncomment the two lines in `cinevault-server.service`:

```ini
LoadCredential=cinevault_key:/etc/cinevault/cinevault.key
Environment=CINEVAULT_SECRET_KEY_FILE=%d/cinevault_key
```

Then `sudo systemctl daemon-reload && sudo systemctl restart cinevault-server`. Back up the key:
without it the encrypted provider keys cannot be read again.

## Tarball

```sh
bun run server:tarball
```

On Linux this writes `target/server-tarball/cinevault-server-<version>-linux-<arch>.tar.gz`
(`amd64` or `arm64`). Unpack it and start it:

```sh
tar -xzf cinevault-server-<version>-linux-amd64.tar.gz
cd cinevault-server-<version>
./run.sh
```

`run.sh` keeps data in `./data`, listens on `127.0.0.1:8787` and prints the address. Reach it from
another machine with `CINEVAULT_WEB_BIND=0.0.0.0 ./run.sh`; choose another data folder with
`CINEVAULT_DATA_DIR=/srv/cinevault ./run.sh`. The tarball also holds `cinevault-server.service` for
a system-wide install.

## First run

1. Open `http://<server>:8787/` in a browser.
2. The welcome page asks for a password of at least 12 characters. It is open for 30 minutes after
   the first start and only until a password exists. No default password is set.
3. In setup, add a library folder with the server's folder browser. The server writes a probe file
   in it and refuses a folder it cannot write.
4. Scan. MediaInfo and FFmpeg download into the data folder on first use; the image does not bundle
   them.

To set or reset the password later, run `cinevault-server passwd` on the host (inside the container:
`docker exec -it cinevault cinevault-server passwd`).

### Deleted files on a bind mount

The freedesktop Recycle Bin is not writable on a bind-mounted media folder. In Settings › Deleted
files choose "trash folder on each drive" instead; CineVault then uses `.deletedByCineVault` beside
the media and can restore from it. Automatic emptying only touches CineVault's own trash folders.
Permanent delete is off by default in a browser and has to be enabled in Settings › Server.

## Update and rollback

Back up the data folder before every upgrade. The important files are `library.db` and
`settings.json`; a copy of the whole folder is simplest when the server is stopped.

```sh
# Container
docker compose pull
docker compose up -d

# Debian / RPM
sudo apt install ./cinevault-server_<new-version>_amd64.deb
# or
sudo dnf upgrade ./cinevault-server-<new-version>-1.x86_64.rpm

# Tarball
# Stop the old copy, unpack the new one beside it, keep CINEVAULT_DATA_DIR pointing at the data.
```

Pulling a new image or installing a new package may migrate `library.db`. Rolling back needs a
same-or-newer build: an older server refuses a library that a newer one migrated. To roll back,
stop the server, restore the pre-upgrade copy of the data folder, then start the older version.

The server never runs the desktop updater and never contacts the desktop update channel. Settings ›
Updates shows the running version and the image or package that updates it.

## Building and checking

| Command | Result |
| --- | --- |
| `bun run server:dev` | Builds the UI and starts the server on the loopback address |
| `bun run server:build` | Builds the release `cinevault-server` program |
| `bun run server:image:build` | Builds the two image builder stages |
| `bun run server:image` | Builds `cinevault-server:local` |
| `bun run server:package` | Builds the `.deb` and `.rpm` (Linux) |
| `bun run server:tarball` | Builds the tarball |

The release workflow publishes the image and attaches the packages and tarball to the GitHub
release. The image tag and the `CINEVAULT_VERSION` build argument are checked against the project
version by `node scripts/check-version.mjs`; the same script checks the package and tarball names
with `--server-artifacts` after they are built.

## Troubleshooting

- **Container is unhealthy:** `docker logs cinevault`. A data folder on an NFS/SMB share is the
  usual cause; move it to local disk.
- **"This folder is not writable by the server":** the folder's UID/GID does not match `PUID`/
  `PGID` (container) or the `cinevault` user (packages). Fix the ownership, not the mount mode.
- **Browser cannot sign in after a restart:** the data folder changed. Set `CINEVAULT_DATA_DIR` to
  the same folder every time; sessions and the password live there.
- **Port already in use:** change the published port (`-p 8788:8787`) and open
  `http://<server>:8788/`. The server itself still listens on 8787 inside the container.
