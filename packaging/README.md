# CineVault in a browser (beta)

CineVault can run as a small server on the computer that holds your movies (a desktop PC, a mini-PC
or a NAS) and be used from a browser on any device in your home: the same library, Scan, Scrape,
Rename, artwork, History and playback in the browser. This is **browser mode**, new in 1.2.0 and
still **beta**. Your files stay where they are; CineVault only uses the folders you give it.

- [Install](#install): one command for Linux, Windows or macOS
- [First visit](#first-visit): password, folders, TMDB key
- [Everyday use](#everyday-use): start, stop, update, forgotten password, logs, uninstall
- [Install by hand](#install-by-hand-with-docker) with `docker run` or Docker Compose
- [NAS](#nas-unraid-synology-truenas), [library folders on a NAS](#library-folders-on-a-nas),
  [Linux packages by hand](#linux-packages-by-hand)
- [Troubleshooting](#troubleshooting), [For maintainers](#for-maintainers)

**Not there yet in the beta:** the server is built for 64-bit Intel and AMD computers only (no ARM,
so not a Raspberry Pi or an ARM NAS); "Open in Kodi" is not offered yet; things a browser cannot do
on your behalf (open a folder in Explorer or Finder, start a desktop video player, hand a torrent to
a desktop app) stay in the desktop app.

## Install

### Linux (Ubuntu, Debian, Fedora, openSUSE and others)

Open a terminal and paste:

```sh
curl -fsSL https://mustafayevt.github.io/CineVault/install.sh | sh
```

It installs CineVault as a normal program and service, like other home media servers: it downloads
the `cinevault-server` package of the latest release (`.deb` on Debian and Ubuntu, `.rpm` on Fedora,
RHEL and openSUSE, a tarball elsewhere), checks its SHA-256, installs it, asks which movie and TV
folders CineVault may use, and starts the `cinevault-server` service, which also starts with the
computer. It asks for your password once (sudo). The service runs as you, so it can write next to
your media; the library lives in `/srv/cinevault`. It needs a 64-bit Intel or AMD computer with
glibc 2.34 or newer (Ubuntu 22.04, Debian 12, Fedora 36 or later).

Prefer Docker on Linux? Add `-s -- --docker`:
`curl -fsSL https://mustafayevt.github.io/CineVault/install.sh | sh -s -- --docker`.

### Windows

1. Install [Docker Desktop](https://docs.docker.com/desktop/setup/install/windows-install/) (free for
   personal use), open it once and accept its terms. Keep Docker Desktop › Settings › General ›
   **Start Docker Desktop when you sign in to your computer** on, so CineVault comes back after a
   restart.
2. Open **PowerShell** (Start menu › type "PowerShell") and paste:

   ```powershell
   irm https://mustafayevt.github.io/CineVault/install.ps1 | iex
   ```

   In the classic Command Prompt (cmd.exe), paste
   `powershell -c "irm https://mustafayevt.github.io/CineVault/install.ps1 | iex"` instead.

### macOS

1. Install [Docker Desktop](https://docs.docker.com/desktop/setup/install/mac-install/), open it once
   and accept its terms, and keep **Start Docker Desktop when you sign in to your computer** on.
2. Open **Terminal** and paste:

   ```sh
   curl -fsSL https://mustafayevt.github.io/CineVault/install.sh | sh
   ```

On Windows and macOS the installer asks for your movie and TV folders and where CineVault may keep
its data (press Enter for `CineVault` in your user folder), makes the key that protects your
provider keys, starts CineVault in Docker and opens it in your browser. It needs no administrator
rights.

### About the installer

It sends nothing anywhere: it only downloads from github.com and ghcr.io. You can read it first:
[install.sh](https://mustafayevt.github.io/CineVault/install.sh) and
[install.ps1](https://mustafayevt.github.io/CineVault/install.ps1) (source:
[`scripts/install/`](../scripts/install)). At the end it prints the address for your phone or
another computer and the folder names to pick in setup.

**Run the same command again** to update CineVault to the newest version, change its folders or
remove it. Your library, settings and password are kept; removing asks before it deletes them.

<details>
<summary>Without questions (for scripts and advanced users)</summary>

`CINEVAULT_YES=1` accepts every default; `CINEVAULT_MEDIA` lists the library folders, separated by
`;`. The actions are `install` (the default), `update`, `folders` and `uninstall`.

```sh
curl -fsSL https://mustafayevt.github.io/CineVault/install.sh | CINEVAULT_YES=1 CINEVAULT_MEDIA="/srv/movies;/srv/tv" sh
curl -fsSL https://mustafayevt.github.io/CineVault/install.sh | sh -s -- update
curl -fsSL https://mustafayevt.github.io/CineVault/install.sh | sh -s -- uninstall
```

```powershell
$env:CINEVAULT_YES = '1'; $env:CINEVAULT_MEDIA = 'D:\Movies;E:\TV'; irm https://mustafayevt.github.io/CineVault/install.ps1 | iex
& ([scriptblock]::Create((irm https://mustafayevt.github.io/CineVault/install.ps1))) update
& ([scriptblock]::Create((irm https://mustafayevt.github.io/CineVault/install.ps1))) uninstall
```

A share in `CINEVAULT_MEDIA` (`//nas/movies`) also needs `CINEVAULT_SHARE_USER` and
`CINEVAULT_SHARE_PASSWORD_FILE` ([Library folders on a NAS](#library-folders-on-a-nas)).
Also: `CINEVAULT_PORT` (default 8787; the next free port is used when it is taken),
`CINEVAULT_NO_BROWSER=1`, `CINEVAULT_VERSION` (Linux packages: a release instead of the latest),
and for Docker `CINEVAULT_DIR` (default `~/cinevault`, on Windows `%USERPROFILE%\CineVault`) and
`CINEVAULT_IMAGE` (for example `ghcr.io/mustafayevt/cinevault:1.2.0`).

</details>

## First visit

1. **Open CineVault.** On the computer it runs on: <http://localhost:8787>. From a phone, tablet or
   another computer: `http://<computer's address>:8787`, for example `http://192.168.1.20:8787`.
   The installer prints this address. To find it yourself:
   - Windows: run `ipconfig` in PowerShell and use the "IPv4 Address" line.
   - macOS: System Settings › Wi-Fi (or Network) › Details, or `ipconfig getifaddr en0` in Terminal.
   - Linux: `hostname -I` (the first address).
2. **Choose a password** (12 characters or more) on the welcome page. It stays open until a
   password is set, but only for browsers on the same computer or your home network; from anywhere
   else it says "Finish setup from your home network". (With Docker's port mapping every client
   looks local to CineVault, so keep the port off the internet.)
3. **Add your folders** in setup with the folder browser.
   - **Linux service:** your folders keep their usual paths (`/srv/media/movies`); the folder
     browser shows the folders you gave the installer.
   - **Docker (Windows, macOS, NAS):** CineVault lives in its own box and sees your folders under
     names that start with `/media/`: your `D:\Movies` is `/media/Movies`. The installer printed the
     list (`/media/<name>    (your folder ...)`). **Do not type `D:\Movies` in CineVault; it will
     not find it.**
4. **Enter your TMDB key.** CineVault identifies titles with your own free
   [TMDB API key](https://www.themoviedb.org/settings/api). It is saved encrypted; the installer made
   the key that protects it (`/etc/cinevault/cinevault.key` for the Linux service, `secret.key`
   beside the data folder for Docker).
5. **Scan.** MediaInfo and FFmpeg download into the data folder the first time they are needed.

Keep the server inside your home network. Never forward its port to the internet; to reach it from
outside, use a VPN. For HTTPS behind a reverse proxy or a tunnel, set `CINEVAULT_WEB_TRUST_PROXY` to
the proxy's address (`127.0.0.1` for a proxy on the same computer): CineVault then takes the
visitor's address from the proxy and marks the sign-in cookie secure. Until it is set, a request
that comes through a proxy counts as outside your home network, so the welcome page and **Forgot
the password?** refuse it, and the log says so once.

## Everyday use

CineVault starts by itself with the computer (the Linux service) or with Docker
(`--restart unless-stopped`). To update, change the folders or remove it, run the install command
again and pick from its menu.

| To | Linux service | Docker (all systems) |
| --- | --- | --- |
| Stop | `sudo systemctl stop cinevault-server` | `docker stop cinevault` |
| Start | `sudo systemctl start cinevault-server` | `docker start cinevault` |
| Restart | `sudo systemctl restart cinevault-server` | `docker restart cinevault` |
| See what it is doing | `journalctl -u cinevault-server -e` | `docker logs --tail 100 cinevault` |
| Where the library is | `/srv/cinevault` | the `data` folder you chose |

**Forgot the password?** On the sign-in page, choose **Forgot the password?** (from a device on your
home network). CineVault writes a one-time PIN to `password-reset-pin.txt` in its data folder; type
it in with a new password. The PIN works once, for 15 minutes, and signs every other browser out.
To read it:

| Linux service | Docker |
| --- | --- |
| `sudo cat /srv/cinevault/password-reset-pin.txt` | `docker exec cinevault cat /data/password-reset-pin.txt` |

Or set a new password directly on the computer CineVault runs on.

Linux service:

```sh
sudo -u "$(systemctl show -p User --value cinevault-server)" env CINEVAULT_DATA_DIR=/srv/cinevault cinevault-server passwd
```

Docker:

```sh
docker exec -it -u cinevault cinevault cinevault-server passwd
```

Or clear it and choose a new one on the welcome page: add `--reset` after `passwd` (for Docker,
without `-it`), restart CineVault as in the table and open the page.

Browsers stay signed in when CineVault restarts or updates (for up to 30 days without use). A new
password, set in Settings, with a PIN or with `passwd`, signs every browser out.

**Your own MediaInfo or FFmpeg.** CineVault downloads its own copies. To use programs already on
the server instead, set their paths on the server itself; a browser cannot choose a program the
server runs. Stop CineVault, open `settings.json` in the data folder and add the path inside its
`"values"` block, for example `"tools.path.ffmpeg": "/usr/bin/ffmpeg"` (or `tools.path.mediainfo`
for the MediaInfo library), then start CineVault again. With Docker, the path is the one inside the
container. The update channel and the player settings, which a browser cannot change either, mean
nothing on the server.

**Daily backups.** While it runs, CineVault keeps a checked copy of the library every day in the
data folder's `backups` folder (`/srv/cinevault/backups`, or `backups` in the Docker `data` folder),
as `library-YYYY-MM-DD.db`, and keeps the last seven. If backups keep failing for three days, the
notification centre says so. To bring one back:

1. Stop CineVault: `sudo systemctl stop cinevault-server` or `docker stop cinevault`.
2. In the data folder, rename `library.db` to `library-before-restore.db`, and move
   `library.db-wal` and `library.db-shm` aside if they are there.
3. Copy `backups/library-YYYY-MM-DD.db` to the data folder as `library.db`, owned by the same user
   as the other files (`sudo cp -p` on Linux; the container fixes the owner of `/data` at start).
4. Start CineVault again.

The library, History and Undo go back to that day. Files renamed or moved after it are found again
by the next scan, but those changes cannot be undone from CineVault. Swap the files the same way to
go back.

**Back up** the library together with its key (`/srv/cinevault` and
`/etc/cinevault/cinevault.key`, or the Docker `data` folder and `secret.key`), ideally while
CineVault is stopped. Without the key your saved provider keys cannot be read again; you would just
enter them again.

**`latest` or a version number?** The installer always installs the newest release. With Docker, the
examples use `ghcr.io/mustafayevt/cinevault:latest`, so updating is "pull and recreate"; to stay on
one version, use its number instead, for example `:1.2.0`, and change it when you choose to update.
Going back to an older version needs the backup from before the update, because a newer version may
upgrade the library.

## Install by hand with Docker

The installer does exactly this; use it if you prefer to see each step. Make a data folder and the
key file once, then start the container. Change only the folder on the left of
`:/media/movies`; add one more `-v` line per extra folder (`-v "E:\TV:/media/tv"`).

**Windows (PowerShell):**

```powershell
New-Item -ItemType Directory -Force "$HOME\CineVault\data" | Out-Null
if (-not (Test-Path "$HOME\CineVault\secret.key")) { $b = New-Object byte[] 32; [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($b); [IO.File]::WriteAllText("$HOME\CineVault\secret.key", [Convert]::ToBase64String($b)) }
docker run -d --name cinevault `
  -p 8787:8787 `
  -v "$HOME\CineVault\data:/data" `
  -v "$HOME\CineVault\secret.key:/run/secrets/cinevault_key:ro" `
  -e CINEVAULT_SECRET_KEY_FILE=/run/secrets/cinevault_key `
  -v "D:\Movies:/media/movies" `
  --restart unless-stopped `
  ghcr.io/mustafayevt/cinevault:latest
```

The backtick at the end of a line continues the command in PowerShell. Command Prompt (cmd.exe)
does not understand it; use PowerShell or the one-command install.

**macOS and Linux (Terminal, bash or zsh):**

```sh
mkdir -p ~/cinevault/data
[ -s ~/cinevault/secret.key ] || openssl rand -base64 32 > ~/cinevault/secret.key
docker run -d --name cinevault \
  -p 8787:8787 \
  -e PUID=$(id -u) -e PGID=$(id -g) \
  -v ~/cinevault/data:/data \
  -v ~/cinevault/secret.key:/run/secrets/cinevault_key:ro \
  -e CINEVAULT_SECRET_KEY_FILE=/run/secrets/cinevault_key \
  -v /path/to/movies:/media/movies \
  --restart unless-stopped \
  ghcr.io/mustafayevt/cinevault:latest
```

The key file is made only once (the second line skips it when it exists): a new key cannot read the
provider keys saved with the old one.

**Update** by hand: download the new image, remove the container and run the same `docker run`
command again. Everything you set up is in the data folder, so nothing is lost:

```sh
docker pull ghcr.io/mustafayevt/cinevault:latest
docker stop cinevault
docker rm cinevault
# then the docker run command above, unchanged
```

**Uninstall:** `docker stop cinevault`, `docker rm cinevault`, then
`docker image rm ghcr.io/mustafayevt/cinevault:latest`. Delete the `CineVault` data folder yourself
if you no longer want the library.

### Docker Compose

[`docker-compose.yml`](../docker-compose.yml) does the same in a file. Put it in an empty folder,
create `secret.key` beside it as its comments show, change the media line to your folder (on Windows
`D:/Movies:/media/movies`), then:

```sh
docker compose up -d                              # start
docker compose pull && docker compose up -d       # update
docker compose down                               # remove (data stays in ./data)
```

## NAS (Unraid, Synology, TrueNAS)

- **Unraid:** the template [`packaging/unraid/cinevault.xml`](unraid/cinevault.xml) fills in the
  container. Copy it to `/boot/config/plugins/dockerMan/templates-user/my-CineVault.xml`, then
  Docker › Add Container › Template: CineVault. Make the Secret key in the Unraid terminal with
  `openssl rand -base64 32`, paste it, keep a copy, and set the Media path to your share.
  PUID 99 and PGID 100 match Unraid's usual share owner.
- **Synology (Container Manager) and TrueNAS SCALE (Apps › Discover › Custom App, Install via
  YAML):** use [`docker-compose.yml`](../docker-compose.yml) as a project. Point the media line at
  your shared folder, put `secret.key` beside the file (or replace the secret with
  `CINEVAULT_SECRET_KEY: <the key>` under `environment`), and set `PUID`/`PGID` to the user that owns
  the share (Synology: `id <user>` over SSH, often 1026 and 100).
- **Other NAS systems with Docker:** the manual `docker run` command above works anywhere the image
  runs (64-bit Intel or AMD).

Keep the data folder on a local disk or the NAS's own volume, not a network share mounted from
elsewhere: the library database does not work reliably over SMB or NFS.

## Library folders on a NAS

Your movies are on a NAS share that needs its own user name and password, and CineVault runs on
another computer? The server cannot connect a share by itself (it runs as an ordinary account, or
in a container), so the installer connects it on the computer CineVault runs on:

1. Run the install command again and choose **2  Change which movie and TV folders CineVault may
   use** (or give the share during the first install).
2. Type the share where it asks for a folder: `//nas/movies`, `\\nas\movies` or `smb://nas/movies`,
   with a folder inside it if you like (`//nas/movies/Films`).
3. Type the share's user (`DOMAIN\name` works too) and its password. The password is not shown, and
   it never appears in a command line or in the installer's output.

The installer connects the share once to check the login and the folder (SMB 3.0 first, then 2.1
for older NAS units) and says plainly what is wrong: a wrong user name or password, a server that
cannot be reached, a share that does not exist, or a folder that is not on it. Then it sets the
share up so it comes back after a restart:

- **Linux:** the share is mounted at `/mnt/<server>-<share>`, for example `/mnt/nas-movies`, by a
  systemd mount unit, and its login is kept in `/etc/cinevault/shares/nas-movies.cred`, which only
  root can read. It asks before installing `cifs-utils`. In CineVault choose **Add folder…** and
  pick `/mnt/nas-movies/Films`. If the NAS is off when the computer starts, CineVault starts anyway
  and the share's folders read offline, with their titles kept; once the NAS is on, mount it again
  with `sudo systemctl start /mnt/nas-movies`.
- **Docker on Linux (`--docker`):** the same mount on the computer, passed into the container as
  `/media/movies` (pick `/media/movies/Films` in CineVault). The password stays in the root-only
  file, not in Docker, and a share mounted after the container started still appears inside it.
- **Docker Desktop (Windows and macOS):** Docker Desktop cannot see the shares your computer is
  connected to, so the installer makes a CIFS volume, `cinevault-nas-movies`, that the container
  sees as `/media/movies`. It is created through Docker's own API, so the password never appears in
  a command line, but Docker keeps it in the volume's options, which anyone who may use Docker on
  that computer can read (`docker volume inspect`). If the NAS is off when the container starts,
  Docker restarts the container until the share can be mounted. Docker resolves the NAS name
  itself; if it cannot (a `.local` name, for example), type the NAS's IP address instead.

When "Change folders" drops a share, or you remove CineVault, the installer removes its mount, its
login file and its volume. It never deletes anything on the NAS, and a mount folder is removed only
once it is no longer mounted.

Without questions, give the user in `CINEVAULT_SHARE_USER` and the password in a file named by
`CINEVAULT_SHARE_PASSWORD_FILE` (its first line; keep the password out of environment variables):

```sh
curl -fsSL https://mustafayevt.github.io/CineVault/install.sh | CINEVAULT_YES=1 CINEVAULT_MEDIA="//nas/movies/Films;/srv/tv" CINEVAULT_SHARE_USER=alex CINEVAULT_SHARE_PASSWORD_FILE=$HOME/nas-password sh
```

<details>
<summary>With Docker Compose</summary>

[`docker-compose.yml`](../docker-compose.yml) has a commented CIFS volume for a share. Uncomment
the `nas-movies` lines, change `//nas/movies` and `addr=nas` to your NAS and share, and keep the
login in an `.env` file beside it that only you can read (`chmod 600 .env`):

```sh
NAS_USER=alex
NAS_PASSWORD='your password'
PUID=1000
PGID=1000
```

Single quotes keep a `$` in the password as it is; write a comma in it twice (`,,`). Docker keeps
the login in the volume's options. Docker creates the volume once: after changing the login, run
`docker compose down`, `docker volume rm <folder>_nas-movies` (removing the volume leaves the files
on the NAS alone) and `docker compose up -d`. On a Linux host you can mount the share on the host
instead (below) and bind it like any folder: `- /mnt/nas-movies:/media/movies:rslave`.

</details>

<details>
<summary>Mount a share by hand on Linux (systemd)</summary>

This is what the installer does. Install `cifs-utils` (`sudo apt install cifs-utils` or
`sudo dnf install cifs-utils`), then make the login file, which only root can read, and type the two
lines `username=alex` and `password=…` into it:

```sh
sudo install -d -m 0700 /etc/cinevault/shares
sudo install -m 0600 /dev/null /etc/cinevault/shares/nas-movies.cred
sudoedit /etc/cinevault/shares/nas-movies.cred
```

The mount unit's name must match its folder: for `/mnt/nas-movies` it is `mnt-nas\x2dmovies.mount`
(`systemd-escape --path --suffix=mount /mnt/nas-movies` prints it). Create it with
`sudoedit '/etc/systemd/system/mnt-nas\x2dmovies.mount'`, with `uid` and `gid` set to the account the
service runs as (`id -u`, `id -g`):

```ini
[Unit]
Description=NAS share //nas/movies
Wants=network-online.target
After=network-online.target

[Mount]
What=//nas/movies
Where=/mnt/nas-movies
Type=cifs
Options=credentials=/etc/cinevault/shares/nas-movies.cred,uid=1000,gid=1000,file_mode=0664,dir_mode=0775,iocharset=utf8,vers=3.0,_netdev,nofail
TimeoutSec=30

[Install]
WantedBy=remote-fs.target
```

Start it with `sudo systemctl daemon-reload` and `sudo systemctl enable --now 'mnt-nas\x2dmovies.mount'`
(use `vers=2.1` for an older NAS). Then, with `sudo systemctl edit cinevault-server`, let the
service want the mount without depending on it, and add the folder to its folder roots:

```ini
[Unit]
Wants=mnt-nas\x2dmovies.mount
After=mnt-nas\x2dmovies.mount

[Service]
Environment="CINEVAULT_WEB_ROOTS=/mnt/nas-movies/Films"
```

</details>

## Linux packages by hand

The Linux installer does these steps for you; follow them to do it yourself or to see what changed.
Each release has a `cinevault-server` `.deb`, an `.rpm` and a tarball for 64-bit Intel and AMD
Linux with glibc 2.34 or newer, on the
[releases page](https://github.com/mustafayevt/CineVault/releases/latest), with their SHA-256 sums in
`SHA256SUMS-Linux.txt`. They are separate from the desktop app's `cinevault` package, so both can be
installed.

```sh
# Debian, Ubuntu
curl -fLO https://github.com/mustafayevt/CineVault/releases/download/v1.2.0/cinevault-server_1.2.0_amd64.deb
sudo apt install ./cinevault-server_1.2.0_amd64.deb

# Fedora, RHEL, openSUSE
curl -fLO https://github.com/mustafayevt/CineVault/releases/download/v1.2.0/cinevault-server-1.2.0-1.x86_64.rpm
sudo dnf install ./cinevault-server-1.2.0-1.x86_64.rpm
```

The package installs `/usr/local/bin/cinevault-server`, the `cinevault` command line, the interface
in `/usr/share/cinevault/ui` and the `cinevault-server` systemd service, and creates the `cinevault`
system user with the library folder `/srv/cinevault`. It does not start the service. Make the key,
then add your settings to the service:

```sh
sudo install -d -m 0755 /etc/cinevault
sudo sh -c 'umask 077; openssl rand -base64 32 > /etc/cinevault/cinevault.key'
sudo systemctl edit cinevault-server
```

In the editor that opens, add these lines and save (the installer writes the same thing to
`/etc/systemd/system/cinevault-server.service.d/cinevault-install.conf`):

```ini
[Service]
LoadCredential=cinevault_key:/etc/cinevault/cinevault.key
Environment=CINEVAULT_SECRET_KEY_FILE=%d/cinevault_key
# Optional: run as the account that owns your media instead of "cinevault".
#User=yourname
#Group=yourname
# Optional: the folders the folder browser may open, separated by ":" (default: /media, /mnt, /srv).
#Environment="CINEVAULT_WEB_ROOTS=/srv/media/movies:/home/yourname/TV"
# Needed when a library folder is under /home.
#ProtectHome=false
```

```sh
sudo chown -R cinevault: /srv/cinevault     # or yourname: when you set User= above
sudo systemctl enable --now cinevault-server
```

Open `http://<server>:8787` and continue with [First visit](#first-visit). The service account must
be able to read and write the library folders (`User=` above, or for example
`sudo chown -R cinevault: /srv/media`).

- **Update:** install the newer package the same way, then `sudo systemctl restart cinevault-server`.
  **Remove:** `sudo apt purge cinevault-server` or `sudo dnf remove cinevault-server`;
  `/srv/cinevault` and `/etc/cinevault` stay until you delete them.
- **Tarball:** unpack `cinevault-server-<version>-linux-amd64.tar.gz` and run `./run.sh` in its
  folder. It keeps data in `./data` and listens on `127.0.0.1:8787` only; use
  `CINEVAULT_WEB_BIND=0.0.0.0 ./run.sh` to reach it from other devices and
  `CINEVAULT_SECRET_KEY_FILE=/path/to/secret.key` for the key. The tarball also holds the systemd
  unit.

## Troubleshooting

- **The Linux installer says this Linux is too old or not supported:** the packages need glibc 2.34
  (Ubuntu 22.04, Debian 12, Fedora 36 or later) on x86_64. Use Docker instead:
  `curl -fsSL https://mustafayevt.github.io/CineVault/install.sh | sh -s -- --docker`.
- **"Provider keys are locked"** when you enter the TMDB key: CineVault was started without its
  key. Use the installer; by hand, add the two `secret.key` lines of the Docker command and recreate
  the container, or the `LoadCredential` lines of [Linux packages by hand](#linux-packages-by-hand). Health then says `"keys":"configured"` (`curl http://localhost:8787/healthz`).
- **"Finish setup from your home network"** instead of the welcome page: the browser is not on
  your home network (or comes through a proxy CineVault doesn't trust). Open the page from a device
  at home, or set the password with the `passwd` command under [Everyday use](#everyday-use).
- **Port 8787 is already in use:** the installer picks the next free port and prints it. By hand,
  change the left number (`-p 8788:8787`) for Docker, or add `Environment=CINEVAULT_WEB_PORT=8788`
  with `sudo systemctl edit cinevault-server`, and open `http://<computer>:8788`.
- **CineVault cannot find my folder / the folder list is empty:** with Docker you are looking for a
  path of your computer (`D:\Movies`, `/Users/me/Movies`); inside CineVault it is `/media/<name>`
  (see [First visit](#first-visit)). A folder you did not give the installer is not visible at all:
  run the install command again and choose 2 to change the folders (by hand: add a `-v` line and
  recreate the container, or extend `CINEVAULT_WEB_ROOTS`).
- **"This folder is not writable by the server":** CineVault writes artwork, NFO files and renames
  next to your media. The Linux service runs as the user who installed it
  (`systemctl show -p User cinevault-server`), who must be able to write the folders. With Docker on
  Linux and NAS systems, start it with `PUID`/`PGID` of the user that owns the files (`id -u`,
  `id -g`; the installer does this). Check that the share is not read-only. On
  Windows and macOS this usually means the folder itself is read-only or on a drive Docker Desktop
  may not use.
- **Windows: a drive or folder is empty inside CineVault:** Docker Desktop with WSL 2 (the default)
  can use every local drive. A mapped network drive (`Z:`) is not visible to Docker; use the NAS's
  own Docker or the share's real path.
- **macOS: an external drive is missing:** Docker Desktop › Settings › Resources › File sharing
  must list `/Volumes` (it does by default).
- **Other devices cannot open the page:** use the computer's address, not `localhost`, and make
  sure both are on the same network. On Windows, allow Docker Desktop through Windows Defender
  Firewall when asked (or in Windows Security › Firewall › Allow an app). On Linux, open the port:
  `sudo ufw allow 8787/tcp` (Ubuntu) or `sudo firewall-cmd --add-port=8787/tcp --permanent && sudo firewall-cmd --reload` (Fedora).
- **Fedora or other SELinux systems:** if CineVault cannot read the media, add `:z` to the media
  `-v` line (`-v /srv/media:/media/media:z`) once; it relabels the folder for containers.
- **It does not come back after a reboot:** on Windows and macOS, Docker Desktop must start when you
  sign in (Settings › General). Linux service: `sudo systemctl enable cinevault-server`; Linux with
  Docker: `sudo systemctl enable docker`.
- **It keeps restarting or is unhealthy:** `journalctl -u cinevault-server -e` or
  `docker logs --tail 50 cinevault` says why.
  A data folder on an SMB or NFS share is the usual cause; keep it on a local disk.
- **Signed out after an update:** the data folder changed, or the password did. Use the same data
  folder every time; the password and the sessions (`server-sessions.json`) live there.
- **"This setting is changed on the computer that runs CineVault":** the update channel, the tool
  paths and the player cannot be changed from a browser. For the tools, see **Your own MediaInfo or
  FFmpeg** under [Everyday use](#everyday-use).

## For maintainers

### Building

| Command | Result |
| --- | --- |
| `bun run server:dev` | Builds the UI and starts the server on the loopback address |
| `bun run server:build` | Builds the release `cinevault-server` program |
| `bun run server:image:build` | Builds the two image builder stages |
| `bun run server:image` | Builds `cinevault-server:local` |
| `bun run server:package` | Builds the `.deb` and `.rpm` (Linux) in `target/server-package/` |
| `bun run server:tarball` | Builds the tarball in `target/server-tarball/` |

The release workflow pushes the image (`:<version>` and `:latest`, amd64) and attaches the
packages and the tarball to the GitHub release; it stops before publishing when one of them is
missing. The image tag and the `CINEVAULT_VERSION` build argument are checked against the project
version by `node scripts/check-version.mjs`, which also checks the package and tarball names with
`--server-artifacts`. The install scripts in `scripts/install/` are published to the website root
by the Deploy site workflow.

### What is where

| Path | Contents |
| --- | --- |
| `/usr/local/bin/cinevault-server` | The browser server |
| `/usr/local/bin/cinevault` | The command line |
| `/usr/share/cinevault/ui` | The built interface |
| `/usr/lib/systemd/system/cinevault-server.service` | The systemd unit ([`cinevault-server.service`](cinevault-server.service)) |

The container keeps its data in `/data` and the library folders under `/media`. Its entrypoint
([`entrypoint.sh`](entrypoint.sh)) maps the image's `cinevault` user to `PUID`/`PGID` (default 1000),
makes `/data` belong to it (only when its top folder has another owner, so a large thumbnail cache
does not slow every start) and replaces itself with the server as that user through `setpriv`, so
no root process is left. Supplementary groups the container is started with (`--group-add 100`, or
`group_add` in Compose) are kept, except group 0, for NAS folders that only a group can reach.
Media folders are not re-owned. Starting with `--user` works too, but then
`PUID`/`PGID` must match it. The image checks `http://127.0.0.1:8787/healthz` every 30 seconds and
stops the server with `SIGINT`, the signal it uses for a clean shutdown.

The master key for the encrypted provider-key file (`secrets.enc` in the data folder) is base64 of
exactly 32 bytes, read from `CINEVAULT_SECRET_KEY_FILE` (a Docker or Compose secret, a mounted file
or a systemd credential) or `CINEVAULT_SECRET_KEY`. Any other value stops the server at start.
Without a key the server still runs and opens the library, health says `"keys":"locked"`, and
provider keys cannot be saved.

### Deleted files on a bind mount

The freedesktop Trash is not writable on a bind-mounted media folder. In Settings › Deleted files
choose "trash folder on each drive"; CineVault then uses `.deletedByCineVault` beside the media and
can restore from it. Automatic emptying only touches CineVault's own trash folders. Permanent delete
is off by default in a browser and is enabled in Settings › Server.

### Rolling back

Pulling a new image or installing a new package may migrate `library.db`. An older server refuses a
library that a newer one migrated: to roll back, stop the server, restore the pre-upgrade copy of
the data folder, then start the older version. The server never runs the desktop updater; Settings ›
Updates shows the running version and how it is updated.
