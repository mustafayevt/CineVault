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
# A library folder can be on a network share: give \\nas\movies (or //nas/movies, smb://nas/movies),
# with a folder inside it if you like (\\nas\movies\Films). Docker Desktop cannot see the shares
# Windows is connected to, so the installer asks for the share's user and password (the password is
# not shown) and makes a CIFS volume, cinevault-<server>-<share>, that the container sees as
# /media/<share>. The volume is created through Docker's own API, so the password never appears in
# a docker command line; Docker keeps it in the volume's options.
#
# Without questions (all optional except CINEVAULT_MEDIA on a first install):
#   $env:CINEVAULT_YES='1'                       accept every default, never ask
#   $env:CINEVAULT_MEDIA='D:\Movies;E:\TV'       library folders, separated by ";" (shares too: \\nas\movies)
#   $env:CINEVAULT_SHARE_USER='alex'             the user for every share in CINEVAULT_MEDIA
#   $env:CINEVAULT_SHARE_PASSWORD_FILE='C:\Users\alex\nas-password.txt'   a file whose first line is that password
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
$S = @{ Binds = @(); Envs = @(); Port = '8787'; LastUser = '' }   # what the container is run with
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
        if ($b -match '^(.*):(/media/[^:]+)(:.*)?$') {
            $media = $Matches[2]; $source = $Matches[1]
            $share = if ($source -notmatch '[\\/]') { Volume-Share $source } else { '' }
            if ($share) { Say ("  {0}    (the network share {1}; its folders are inside)" -f $media, ($share -replace '/', '\')) }
            else { Say ("  {0}    (your folder {1})" -f $media, $source) }
        }
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
    if (Is-Share $Folder) { return Add-Share $Folder }
    if (-not $Folder -or -not (Test-Path -LiteralPath $Folder -PathType Container)) {
        Say "  There is no folder '$Folder'."
        return $false
    }
    $full = (Resolve-Path -LiteralPath $Folder).ProviderPath.TrimEnd('\')
    if ($full -match '^[A-Za-z]:$') { $full += '\' }
    $S.Binds += "$($full):/media/$(Mount-Name $full)"
    return $true
}

# ---- network shares -----------------------------------------------------------------------------
# The password is read with Read-Host -AsSecureString (or from the first line of
# CINEVAULT_SHARE_PASSWORD_FILE) and only ever leaves this window in the body sent to Docker's API.
function Is-Share([string]$Text) { return $Text -match '^(\\\\|//|smb://)' }

function Share-Key([string]$Server, [string]$Share) {
    return (("$Server-$Share").ToLowerInvariant() -replace '[^a-z0-9._-]', '-').TrimEnd('-')
}

function Parse-Share([string]$Address) {
    $a = ($Address -replace '\\', '/') -replace '^smb://', '' -replace '^//', ''
    $a = ($a -replace '/+', '/').TrimEnd('/')
    $parts = @($a -split '/', 3)
    $server = $parts[0]
    $hint = ''   # the user in smb://alex@nas/movies
    if ($server -match '^(.*)@([^@]*)$') { $hint = $Matches[1]; $server = $Matches[2] }
    $share = if ($parts.Count -gt 1) { $parts[1] } else { '' }
    $sub = if ($parts.Count -gt 2) { $parts[2] } else { '' }
    if ($server -cnotmatch '^[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?$') {
        Say "  $Address does not start with a server name or IPv4 address, like \\nas\movies."
        return $null
    }
    if (-not $share) { Say "  $Address names no share. Type it like \\$server\movies."; return $null }
    if ($share -cnotmatch '^[A-Za-z0-9._$ -]{1,80}$') {
        Say "  The share name ""$share"" has a sign the installer cannot use (letters, digits, spaces, . _ - and `$ are fine)."
        return $null
    }
    if ("/$sub/" -match '/\.\.?/') { Say '  The folder inside the share may not have . or .. in its path.'; return $null }
    if ($sub -match '[\x00-\x1f"]') { Say '  The folder inside the share has a sign the installer cannot use.'; return $null }
    return @{ Server = $server; Share = $share; Sub = $sub; Key = (Share-Key $server $share); Hint = $hint }
}

function Share-Kind([string]$Output) {
    if ($Output -match 'error\(13\)|permission denied|logon failure') { return 'login' }
    if ($Output -match 'error\(2\)|no such file') { return 'missing' }
    if ($Output -match 'resolve') { return 'noname' }
    return 'other'
}

function Mask([string]$Text) { return $Text -replace 'password=.*,uid=', 'password=********,uid=' }

function Share-Error($Sh, [string]$Output) {
    $name = "\\$($Sh.Server)\$($Sh.Share)"
    $unreachable = "$($Sh.Server) could not be reached. Check the name or address and that it is switched on."
    switch (Share-Kind $Output) {
        'login' { return "Wrong user name or password for $name." }
        'missing' { return "There is no share named $($Sh.Share) on $($Sh.Server)." }
        'noname' { return $unreachable }
    }
    if ($Output -match 'error\((110|111|112|113)\)|host is down|no route|timed out|unreachable') { return $unreachable }
    $line = @($Output -split "`r?`n" | Where-Object { $_ -match 'error' } | Select-Object -First 1)
    return "$name could not be mounted: $(Mask ([string]$line))"
}

function Plain([Security.SecureString]$Secure) {
    $ptr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($Secure)
    try { return [Runtime.InteropServices.Marshal]::PtrToStringBSTR($ptr) } finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr) }
}

function Ask-Login($Sh) {
    $name = "\\$($Sh.Server)\$($Sh.Share)"
    if ($Yes -or $env:CINEVAULT_SHARE_PASSWORD_FILE) {
        $user = if ($env:CINEVAULT_SHARE_USER) { $env:CINEVAULT_SHARE_USER } else { $Sh.Hint }
        $file = $env:CINEVAULT_SHARE_PASSWORD_FILE
        if (-not $user -or -not $file) {
            Fail "For the share $name, set CINEVAULT_SHARE_USER and CINEVAULT_SHARE_PASSWORD_FILE (a file whose first line is the password)."
        }
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { Fail "The password file $file cannot be read." }
        $secure = New-Object Security.SecureString
        foreach ($c in ([string](Get-Content -LiteralPath $file -TotalCount 1)).ToCharArray()) { $secure.AppendChar($c) }
    } else {
        $default = if ($env:CINEVAULT_SHARE_USER) { $env:CINEVAULT_SHARE_USER } elseif ($Sh.Hint) { $Sh.Hint } else { $S.LastUser }
        do { $user = Ask "User for $name" $default } until ($user)
        $secure = Read-Host -AsSecureString "Password for $user on $($Sh.Server) (it is not shown)"
    }
    if ($user -match '[,=\x00-\x1f]') { Fail "The user name for $name cannot have , or = or control characters." }
    $S.LastUser = $user
    return @{ User = $user; Password = $secure }
}

function Volume-Body([string]$Name, $Sh, [string]$User, [string]$Password, [string]$Vers, [string]$Uid, [string]$Gid) {
    $u = $User
    $domain = ''
    if ($u -match '^(.*?)\\(.*)$') { $domain = ",domain=$($Matches[1])"; $u = $Matches[2] }
    # A comma in a mount option value is written twice.
    $o = "addr=$($Sh.Server),username=$u$domain,password=$($Password -replace ',', ',,'),uid=$Uid,gid=$Gid,file_mode=0664,dir_mode=0775,vers=$Vers"
    $device = "//$($Sh.Server)/$($Sh.Share)"
    $body = [ordered]@{
        Name = $Name
        Driver = 'local'
        DriverOpts = [ordered]@{ type = 'cifs'; device = $device; o = $o }
        Labels = [ordered]@{ 'cinevault.share' = $device; 'cinevault.key' = $Sh.Key }
    }
    return ($body | ConvertTo-Json -Compress -Depth 4)
}

function Docker-Pipe {
    $endpoint = $env:DOCKER_HOST
    if (-not $endpoint) {
        try { $endpoint = @(docker context inspect | Out-String | ConvertFrom-Json)[0].Endpoints.docker.Host } catch { }
    }
    if ($endpoint -match '^npipe://.*/pipe/(.+)$') { return $Matches[1] }
    return 'docker_engine'
}

# Sends one request to the Docker Engine API over its named pipe, in this process, so the body never
# appears in a command line.
function Docker-Api([string]$Method, [string]$Path, [string]$Body) {
    $pipe = New-Object IO.Pipes.NamedPipeClientStream('.', (Docker-Pipe), [IO.Pipes.PipeDirection]::InOut)
    try {
        $pipe.Connect(10000)
        $data = [Text.Encoding]::UTF8.GetBytes($Body)
        $head = [Text.Encoding]::ASCII.GetBytes("$Method $Path HTTP/1.1`r`nHost: docker`r`nContent-Type: application/json`r`nContent-Length: $($data.Length)`r`nConnection: close`r`n`r`n")
        $pipe.Write($head, 0, $head.Length)
        $pipe.Write($data, 0, $data.Length)
        $pipe.Flush()
        [Array]::Clear($data, 0, $data.Length)
        $reply = (New-Object IO.StreamReader($pipe, [Text.Encoding]::UTF8)).ReadToEnd()
    } finally {
        $pipe.Dispose()
    }
    $code = 0
    if ($reply -match '^HTTP/\S+ (\d{3})') { $code = [int]$Matches[1] }
    return @{ Code = $code; Text = @($reply -split "`r`n`r`n", 2)[-1] }
}

function Volume-Test([string]$Vol, [string]$Sub) {
    $target = if ($Sub) { "/m/$Sub" } else { '/m' }
    $out = (& docker run --rm -v "$($Vol):/m" --entrypoint test $Image -d $target 2>&1 | Out-String)
    if ($LASTEXITCODE -eq 0) { return @{ Kind = 'ok'; Out = '' } }
    if ($LASTEXITCODE -eq 1) { return @{ Kind = 'folder'; Out = '' } }
    return @{ Kind = (Share-Kind $out); Out = $out }
}

function Free-Volume([string]$Base) {
    $candidate = $Base
    $n = 2
    while ($true) {
        docker volume inspect $candidate *> $null
        if ($LASTEXITCODE -ne 0) { return $candidate }
        $candidate = "$Base-$n"
        $n++
    }
}

function Container-Ids {
    $uid = '1000'; $gid = '1000'
    foreach ($e in $S.Envs) {
        if ($e -match '^PUID=(\d+)$') { $uid = $Matches[1] }
        if ($e -match '^PGID=(\d+)$') { $gid = $Matches[1] }
    }
    return @($uid, $gid)
}

# A CIFS volume for the share, checked by mounting it in a throwaway container. Returns its name.
function Share-Volume($Sh) {
    docker image inspect $Image *> $null
    if ($LASTEXITCODE -ne 0) { Pull | Out-Host }
    $name = "\\$($Sh.Server)\$($Sh.Share)"
    $vol = @(docker volume ls -q --filter "label=cinevault.key=$($Sh.Key)") | Select-Object -First 1
    if ($vol) {
        $t = Volume-Test $vol $Sh.Sub
        if ($t.Kind -eq 'ok') { Say "  $name is the volume $vol already, with the login saved before."; return $vol }
        if ($t.Kind -eq 'folder') { Say "  $($Sh.Sub) was not found on $name."; return $null }
        Say "  The volume $vol no longer mounts. $(Share-Error $Sh $t.Out)"
    }
    $ids = Container-Ids
    for ($tries = 1; ; $tries++) {
        $login = Ask-Login $Sh
        $vol = Free-Volume "cinevault-$($Sh.Key)"
        $t = @{ Kind = 'other'; Out = '' }
        # SMB 3.0 first; some older NAS units only speak 2.1.
        foreach ($vers in '3.0', '2.1') {
            $reply = Docker-Api 'POST' '/volumes/create' (Volume-Body $vol $Sh $login.User (Plain $login.Password) $vers $ids[0] $ids[1])
            if ($reply.Code -lt 200 -or $reply.Code -ge 300) { Fail "Docker could not make the volume $($vol): $(Mask $reply.Text)" }
            $t = Volume-Test $vol $Sh.Sub
            if ($t.Kind -eq 'ok') { break }
            docker volume rm $vol *> $null
            if ($t.Kind -ne 'other') { break }
        }
        $login.Password.Dispose()
        if ($t.Kind -eq 'ok') {
            Say "  $name is the volume $vol. Docker keeps its login in the volume's options, which anyone"
            Say '  who may use Docker on this computer can read.'
            return $vol
        }
        if ($t.Kind -eq 'folder') { Say "  $($Sh.Sub) was not found on $name."; return $null }
        Say "  $(Share-Error $Sh $t.Out)"
        if ($t.Kind -ne 'login' -or $Yes -or $env:CINEVAULT_SHARE_PASSWORD_FILE -or $tries -ge 3) { return $null }
    }
}

function Add-Share([string]$Address) {
    $sh = Parse-Share $Address
    if (-not $sh) { return $false }
    $vol = Share-Volume $sh
    if (-not $vol) { return $false }
    if (-not ($S.Binds | Where-Object { $_ -like "$($vol):*" })) { $S.Binds += "$($vol):/media/$(Mount-Name $sh.Share)" }
    return $true
}

function Volume-Share([string]$Vol) { # the share a volume made by this installer holds, or ''
    try { return [string]@(docker volume inspect $Vol 2>$null | Out-String | ConvertFrom-Json)[0].Labels.'cinevault.share' } catch { return '' }
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

# install.ps1 -SelfTest: checks the share parsing and the body sent to Docker, without Docker.
function Self-Test {
    $T = @{ Fails = 0 }
    function Expect([string]$What, $Got, $Want) {
        if ("$Got" -cne "$Want") { Write-Host "FAIL $What`n  expected: $Want`n  got:      $Got"; $T.Fails++ }
    }
    function Parsed([string]$Address) {
        $p = Parse-Share $Address 6>$null
        if (-not $p) { return 'refused' }
        return '{0}|{1}|{2}|{3}|{4}' -f $p.Server, $p.Share, $p.Sub, $p.Key, $p.Hint
    }
    Expect 'share \\nas\movies' (Is-Share '\\nas\movies') $true
    Expect 'share //nas/movies' (Is-Share '//nas/movies') $true
    Expect 'share smb://nas/movies' (Is-Share 'smb://nas/movies') $true
    Expect 'share SMB://nas/movies' (Is-Share 'SMB://nas/movies') $true
    Expect 'local D:\Movies' (Is-Share 'D:\Movies') $false
    Expect 'local Movies' (Is-Share 'Movies') $false

    Expect 'parse \\nas\movies' (Parsed '\\nas\movies') 'nas|movies||nas-movies|'
    Expect 'parse \\NAS\Movies\Films\HD' (Parsed '\\NAS\Movies\Films\HD') 'NAS|Movies|Films/HD|nas-movies|'
    Expect 'parse //nas/movies/Films/' (Parsed '//nas/movies/Films/') 'nas|movies|Films|nas-movies|'
    Expect 'parse smb://alex@nas.local/movies' (Parsed 'smb://alex@nas.local/movies') 'nas.local|movies||nas.local-movies|alex'
    Expect 'parse smb://192.168.1.5/Media Library/TV' (Parsed 'smb://192.168.1.5/Media Library/TV') '192.168.1.5|Media Library|TV|192.168.1.5-media-library|'
    Expect 'parse \\nas\movies$' (Parsed '\\nas\movies$') 'nas|movies$||nas-movies|'
    Expect 'refuse \\nas' (Parsed '\\nas') 'refused'
    Expect 'refuse \\-nas\movies' (Parsed '\\-nas\movies') 'refused'
    Expect 'refuse \\nas\mo*vies' (Parsed '\\nas\mo*vies') 'refused'
    Expect 'refuse \\nas\movies\..\x' (Parsed '\\nas\movies\..\x') 'refused'

    $sh = Parse-Share '\\nas\movies'
    $body = Volume-Body 'cinevault-nas-movies' $sh 'WORK\alex' 'a,b"c\d' '3.0' '1000' '1000' | ConvertFrom-Json
    Expect 'volume name' $body.Name 'cinevault-nas-movies'
    Expect 'volume driver' $body.Driver 'local'
    Expect 'volume type' $body.DriverOpts.type 'cifs'
    Expect 'volume device' $body.DriverOpts.device '//nas/movies'
    Expect 'volume options' $body.DriverOpts.o 'addr=nas,username=alex,domain=WORK,password=a,,b"c\d,uid=1000,gid=1000,file_mode=0664,dir_mode=0775,vers=3.0'
    Expect 'volume share label' $body.Labels.'cinevault.share' '//nas/movies'
    Expect 'volume key label' $body.Labels.'cinevault.key' 'nas-movies'

    $secure = New-Object Security.SecureString
    foreach ($c in 'Pa,ss w"rd'.ToCharArray()) { $secure.AppendChar($c) }
    Expect 'password from the secure string' (Plain $secure) 'Pa,ss w"rd'

    Expect 'kind wrong login' (Share-Kind 'failed to mount local volume: mount //nas/movies:/var/lib/docker/volumes/v/_data, data: addr=nas: permission denied') 'login'
    Expect 'kind no share' (Share-Kind 'failed to mount local volume: mount //nas/x:/var/lib/docker/volumes/v/_data: no such file or directory') 'missing'
    Expect 'kind other' (Share-Kind 'failed to mount local volume: invalid argument') 'other'
    Expect 'password masked' (Mask 'data: addr=nas,username=alex,password=se,,cret,uid=1000,gid=1000: permission denied') 'data: addr=nas,username=alex,password=********,uid=1000,gid=1000: permission denied'
    Expect 'error wrong login' (Share-Error $sh 'mount error(13): Permission denied') 'Wrong user name or password for \\nas\movies.'
    Expect 'error host down' (Share-Error $sh 'mount: host is down') 'nas could not be reached. Check the name or address and that it is switched on.'

    $S.Binds = @()
    Expect 'mount name of a share' (Mount-Name 'Media Library') 'Media-Library'
    $S.Binds = @('cinevault-nas-movies:/media/movies')
    Expect 'second share named movies' (Mount-Name 'movies') 'movies-2'
    $S.Binds = @()

    if ($T.Fails -eq 0) { Write-Host 'install.ps1 self-test passed' } else { Write-Host "install.ps1 self-test: $($T.Fails) failed"; exit 1 }
}

if ($Action -match '^(-SelfTest|self-test)$') { Self-Test; return }

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
