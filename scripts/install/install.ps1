# CineVault browser mode (beta): install, update or remove the server with Docker Desktop on Windows.
#
#   irm https://mustafayevt.github.io/CineVault/install.ps1 | iex          install or update
#   & ([scriptblock]::Create((irm https://mustafayevt.github.io/CineVault/install.ps1))) uninstall
#
# What it does, in order: checks that Docker Desktop is installed and running (and starts it if
# needed), asks for your movie and TV folders and a data folder, makes the key that protects your
# provider keys, downloads the ghcr.io/mustafayevt/cinevault image, starts a container named
# "cinevault" that comes back after a restart, waits until it answers and opens it in the browser.
# Run it again to update; your library, settings and password stay in the data folder. It sends
# nothing anywhere except the image download from ghcr.io. It needs no administrator rights.
#
# Without questions (all optional except CINEVAULT_MEDIA on a first install):
#   $env:CINEVAULT_YES='1'                       accept every default, never ask
#   $env:CINEVAULT_MEDIA='D:\Movies;E:\TV'       library folders, separated by ";"
#   $env:CINEVAULT_DIR="$HOME\CineVault"         where the data folder and the key file go
#   $env:CINEVAULT_PORT='8787'                   the port to open in the browser
#   $env:CINEVAULT_IMAGE='ghcr.io/mustafayevt/cinevault:latest'
#   $env:CINEVAULT_NO_BROWSER='1'                do not open a browser at the end
#   $env:CINEVAULT_ACTION='install' | 'update' | 'folders' | 'uninstall'   (or the first argument)
#
# Everything runs inside one script block so nothing is left in your PowerShell session, and errors
# end the script, never the window.
& {
$Name = 'cinevault'
$Image = if ($env:CINEVAULT_IMAGE) { $env:CINEVAULT_IMAGE } else { 'ghcr.io/mustafayevt/cinevault:latest' }
$Action = if ($args.Count -gt 0) { [string]$args[0] } elseif ($env:CINEVAULT_ACTION) { $env:CINEVAULT_ACTION } else { 'install' }
$Yes = [bool]$env:CINEVAULT_YES
$S = @{ Binds = @(); Envs = @(); Port = '8787' }   # what the container is run with
$KeepIn = 'CINEVAULT_SECRET_KEY|CINEVAULT_SECRET_KEY_FILE|PUID|PGID|TZ|CINEVAULT_WEB_TRUST_PROXY|CINEVAULT_WEB_ROOTS'

function Say([string]$Text) { Write-Host $Text }
function Step([string]$Text) { Write-Host ''; Write-Host "==> $Text" -ForegroundColor Cyan }
function Fail([string]$Text) { throw $Text }
function Ask([string]$Question, [string]$Default) {
    if ($Yes) { return $Default }
    $prompt = if ($Default) { "$Question [$Default]" } else { $Question }
    $answer = Read-Host $prompt
    if ([string]::IsNullOrWhiteSpace($answer)) { return $Default }
    return $answer.Trim().Trim('"').Trim("'")
}
function Exists { docker container inspect $Name *> $null; return $LASTEXITCODE -eq 0 }
function DockerReady { docker info *> $null; return $LASTEXITCODE -eq 0 }

function Ensure-Docker {
    Step 'Checking Docker Desktop'
    if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
        Say 'Docker Desktop is not installed. It is free for personal use:'
        Say '  https://docs.docker.com/desktop/setup/install/windows-install/'
        if (Get-Command winget -ErrorAction SilentlyContinue) {
            Say 'It can also be installed with:  winget install -e --id Docker.DockerDesktop'
        }
        Fail 'Install Docker Desktop, start it once and accept its terms, then run this command again.'
    }
    if (DockerReady) { return }
    $exe = Join-Path $env:ProgramFiles 'Docker\Docker\Docker Desktop.exe'
    if (Test-Path $exe) {
        Say 'Starting Docker Desktop (this can take a minute)...'
        Start-Process $exe
    }
    for ($i = 0; $i -lt 90; $i++) {
        if (DockerReady) { return }
        Start-Sleep -Seconds 2
    }
    Fail 'Docker Desktop is not running. Start it from the Start menu, wait until it says "Engine running", then run this again.'
}

# Windows PowerShell 5.1 drops the double quotes inside native arguments, so docker inspect's JSON
# is read here instead of passing it templates with quoted keys.
function Inspect { return @(docker inspect $Name | Out-String | ConvertFrom-Json)[0] }

function Read-Existing {
    $info = Inspect
    $S.Binds = @($info.HostConfig.Binds | Where-Object { $_ })
    $S.Envs = @($info.Config.Env | Where-Object { $_ -match "^($KeepIn)=" })
    $S.Port = '8787'
    $bindings = $info.HostConfig.PortBindings.'8787/tcp'
    if ($bindings -and @($bindings)[0].HostPort) { $S.Port = [string]@($bindings)[0].HostPort }
}

function Run-Container {
    $run = @('run', '-d', '--name', $Name, '-p', "$($S.Port):8787", '--restart', 'unless-stopped', '--stop-timeout', '30')
    foreach ($b in $S.Binds) { $run += @('-v', $b) }
    foreach ($e in $S.Envs) { $run += @('-e', $e) }
    docker @run $Image | Out-Null
    if ($LASTEXITCODE -ne 0) { Fail 'The container could not be started; the message above says why.' }
}

function Pull {
    Step "Downloading $Image"
    docker pull $Image
    if ($LASTEXITCODE -ne 0) { Fail 'The image could not be downloaded. Check the internet connection and try again.' }
}

function Wait-Healthy {
    Step 'Waiting for CineVault to start'
    for ($i = 0; $i -lt 60; $i++) {
        try {
            Invoke-WebRequest -UseBasicParsing -TimeoutSec 3 "http://localhost:$($S.Port)/healthz" | Out-Null
            return
        } catch { }
        $state = docker inspect -f '{{.State.Running}} {{.RestartCount}}' $Name 2>$null
        if ($state -ne 'true 0') { break }
        Start-Sleep -Seconds 2
    }
    docker logs --tail 20 $Name
    Fail "CineVault did not start. The lines above say why; 'docker logs cinevault' shows more."
}

function Lan-Ip {
    try {
        $config = Get-NetIPConfiguration -ErrorAction Stop |
            Where-Object { $_.IPv4DefaultGateway -and $_.NetAdapter.Status -eq 'Up' } | Select-Object -First 1
        if ($config) { return @($config.IPv4Address)[0].IPAddress }
    } catch { }
    return $null
}

function Finish([bool]$Fresh) {
    $url = "http://localhost:$($S.Port)/"
    $ip = Lan-Ip
    Say ''
    Write-Host 'CineVault is running.' -ForegroundColor Green
    Say "  On this computer:      $url"
    if ($ip) { Say "  From another device:   http://$($ip):$($S.Port)/  (allow Docker Desktop in Windows Firewall if asked)" }
    Say ''
    if ($Fresh) {
        Say 'First visit: choose a password on the welcome page (from any device at home).'
        Say 'Then add your folders in setup:'
    } else {
        Say 'Sign in with your password. Your folders, as CineVault sees them:'
    }
    foreach ($b in $S.Binds) {
        if ($b -match '^(.*):(/media/[^:]+)(:.*)?$') { Say ("  {0}    (your folder {1})" -f $Matches[2], $Matches[1]) }
    }
    Say ''
    Say 'Keep Docker Desktop starting with Windows (Docker Desktop > Settings > General >'
    Say '"Start Docker Desktop when you sign in to your computer") so CineVault comes back after a restart.'
    Say 'Run the same command again to update CineVault, change its folders or remove it.'
    if (-not $env:CINEVAULT_NO_BROWSER) { Start-Process $url }
}

function Not-Compose {
    if ((Inspect).Config.Labels.'com.docker.compose.project') {
        Fail 'This CineVault was started with Docker Compose. Change it in its docker-compose.yml, and update it from that folder: docker compose pull, then docker compose up -d'
    }
}

function Recreate {
    Step 'Replacing the container (your data folder is kept)'
    docker stop $Name *> $null
    docker rm $Name | Out-Null
    Run-Container
    Wait-Healthy
    Finish $false
}

function Update {
    Not-Compose
    Read-Existing
    Pull
    Recreate
}

function Change-Folders {
    Not-Compose
    Read-Existing
    Say 'CineVault now sees these folders:'
    $S.Binds | Where-Object { $_ -match ':/media/' } | ForEach-Object { Say "  $_" }
    $keep = @($S.Binds | Where-Object { $_ -notmatch ':/media/' })
    $S.Binds = @()
    Ask-Media
    $S.Binds = $keep + $S.Binds
    Recreate
}

function Uninstall {
    if (-not (Exists)) { Say "CineVault is not installed (no container named $Name)."; return }
    Read-Existing
    $data = $S.Binds | Where-Object { $_ -match ':/data$' } | ForEach-Object { $_ -replace ':/data$', '' } | Select-Object -First 1
    $ok = Ask "Remove the CineVault container? Your data folder ($data) is kept. (y/n)" 'y'
    if ($ok -notmatch '^[Yy]') { Say 'Nothing changed.'; return }
    docker stop $Name *> $null
    docker rm $Name | Out-Null
    docker image rm $Image *> $null
    Say 'CineVault is removed. Your movie folders were not touched.'
    if ($data -and -not $Yes -and (Ask 'Type DELETE to also erase the data folder (library, password, keys), or press Enter to keep it' '') -ceq 'DELETE') {
        Remove-Item -Recurse -Force -LiteralPath $data
        $key = $S.Binds | Where-Object { $_ -match ':/run/secrets/cinevault_key' } | ForEach-Object { $_ -replace ':/run/secrets/cinevault_key.*$', '' } | Select-Object -First 1
        if ($key -and (Test-Path -LiteralPath $key -PathType Leaf)) { Remove-Item -Force -LiteralPath $key }
        $parent = Split-Path -Parent $data
        if ($parent -and -not (Get-ChildItem -Force -LiteralPath $parent)) { Remove-Item -LiteralPath $parent }
        Say "Deleted $data and its key."
    } elseif ($data) {
        Say "The data folder is still at $data; running the installer again picks it up."
    }
}

function Mount-Name([string]$Folder) {
    $base = (Split-Path -Leaf $Folder) -replace '[^A-Za-z0-9._-]', '-' -replace '^-+|-+$', ''
    if (-not $base) { $base = ($Folder -replace '[^A-Za-z0-9]', '') }
    if (-not $base) { $base = 'media' }
    $candidate = $base
    $n = 2
    while ($S.Binds -match ":/media/$([regex]::Escape($candidate))$") { $candidate = "$base-$n"; $n++ }
    return $candidate
}

function Add-Media([string]$Folder) {
    $Folder = $Folder.Trim().Trim('"').Trim("'")
    if (-not $Folder -or -not (Test-Path -LiteralPath $Folder -PathType Container)) {
        Say "  There is no folder '$Folder'."
        return $false
    }
    $full = (Resolve-Path -LiteralPath $Folder).ProviderPath.TrimEnd('\')
    if ($full -match '^[A-Za-z]:$') { $full += '\' }
    $S.Binds += "$($full):/media/$(Mount-Name $full)"
    return $true
}

function Port-Free([int]$P) {
    $client = New-Object Net.Sockets.TcpClient
    try { $client.Connect('127.0.0.1', $P); return $false } catch { return $true } finally { $client.Close() }
}

function Ask-Media {
    Step 'Your movie and TV folders'
    if ($env:CINEVAULT_MEDIA) {
        foreach ($f in ($env:CINEVAULT_MEDIA -split ';' | Where-Object { $_.Trim() })) {
            if (-not (Add-Media $f)) { Fail 'Fix CINEVAULT_MEDIA and run this again.' }
        }
    } else {
        if ($Yes) { Fail 'Set CINEVAULT_MEDIA to your library folders, for example $env:CINEVAULT_MEDIA=''D:\Movies;E:\TV''.' }
        Say 'CineVault reads and renames the files in these folders, and writes artwork and NFO files next to them.'
        do { $f = Ask 'Folder with your movies or TV shows (for example D:\Movies)' '' } until ($f -and (Add-Media $f))
        while ($true) {
            $f = Ask 'Another folder? Type its path, or press Enter to go on' ''
            if (-not $f) { break }
            Add-Media $f | Out-Null
        }
    }
}

function Install {
    Say 'CineVault browser mode (beta) installer'
    $S.Binds = @()
    Ask-Media

    Step 'Where CineVault keeps its own data'
    $default = if ($env:CINEVAULT_DIR) { $env:CINEVAULT_DIR } else { Join-Path $HOME 'CineVault' }
    $dir = Ask 'Data folder (library, settings, password)' $default
    New-Item -ItemType Directory -Force -Path (Join-Path $dir 'data') | Out-Null
    $dir = (Resolve-Path -LiteralPath $dir).ProviderPath.TrimEnd('\')
    $key = Join-Path $dir 'secret.key'
    if (-not (Test-Path -LiteralPath $key) -or (Get-Item -LiteralPath $key).Length -eq 0) {
        # The key that encrypts your TMDB and other provider keys. Keep it with the data folder.
        $bytes = New-Object byte[] 32
        [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
        [IO.File]::WriteAllText($key, [Convert]::ToBase64String($bytes))
    }
    $S.Binds = @("$dir\data:/data", "$($key):/run/secrets/cinevault_key:ro") + $S.Binds
    $S.Envs = @('CINEVAULT_SECRET_KEY_FILE=/run/secrets/cinevault_key')

    $S.Port = if ($env:CINEVAULT_PORT) { $env:CINEVAULT_PORT } else { '8787' }
    $p = [int]$S.Port
    while (-not (Port-Free $p)) { $p++; if ($p -ge 8800) { Fail 'No free port near 8787. Set $env:CINEVAULT_PORT.' } }
    if ("$p" -ne $S.Port) { Say "Port $($S.Port) is in use, so CineVault will use port $p." ; $S.Port = "$p" }

    Pull
    Step 'Starting CineVault'
    Run-Container
    Wait-Healthy
    Finish $true
}

try {
    Ensure-Docker
    switch -Regex ($Action) {
        '^(uninstall|remove)$' { Uninstall; break }
        '^folders$' { if (-not (Exists)) { Fail "CineVault is not installed yet. Run the installer without 'folders'." }; Change-Folders; break }
        '^update$' { if (-not (Exists)) { Fail "CineVault is not installed yet. Run the installer without 'update'." }; Update; break }
        '^install$' {
            if (Exists) {
                Say 'CineVault is already installed.'
                Say '  1  Update to the newest version (keeps your library, settings and password)'
                Say '  2  Change which movie and TV folders CineVault sees'
                Say '  3  Remove CineVault (keeps your data folder unless you ask)'
                Say '  4  Stop here'
                $choice = Ask 'Type a number' '1'
                switch ($choice) { '1' { Update } '2' { Change-Folders } '3' { Uninstall } default { Say 'Nothing changed.' } }
            } else {
                Install
            }
            break
        }
        default { Fail "Unknown action '$Action'. Use install, update, folders or uninstall." }
    }
} catch {
    Write-Host ''
    Write-Host "Error: $($_.Exception.Message)" -ForegroundColor Red
}
} @args
