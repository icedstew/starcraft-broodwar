# host_web.ps1: host a networked game from the scbw exe AND serve the browser page, so a
# browser can join. Run from anywhere:
#   powershell -ExecutionPolicy Bypass -File "C:\src\scbw reverse\scbw\agent\tools\host_web.ps1"
#   ... host_web.ps1 -Map "(4)Dark Crystal.scm" -Players 3 -Delay 2 -Lan
# It prints the join code and a ready-made browser URL; Ctrl+C / closing the game stops
# the web server too. The browser must open the page from localhost (WebTransport needs a
# secure context), so for another machine run the web server there (or serve over https).
param(
    [string]$Map = "(2)Bottleneck.scm",      # a file name in <install>\maps, or a full path
    [int]$Port = 4433,                       # game (UDP / QUIC) port
    [int]$Players = 2,                       # humans in total, including this window
    [int]$Delay = 2,                         # input delay in frames
    [int]$WebPort = 8000,                    # the browser page
    [switch]$Lan,                            # listen on every interface; the code gets this PC's LAN IP
    [switch]$NoWeb,                          # do not start the page server
    [string]$Exe = ""                        # scbw.exe to run (default: the newest build found)
)
$ErrorActionPreference = "Stop"
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path

if (-not $env:SCBW_INSTALL) { $env:SCBW_INSTALL = (Join-Path $repo "..\reference\bw install\sc-1.16.1") }
$install = (Resolve-Path $env:SCBW_INSTALL).Path
$env:SCBW_INSTALL = $install

if (-not $Exe) {
    $cands = @("agent\local\scratch\bin\scbw_host.exe", "agent\local\tgt2\release\scbw.exe",
               "agent\local\tgt\release\scbw.exe", "target\release\scbw.exe") |
        ForEach-Object { Join-Path $repo $_ } | Where-Object { Test-Path $_ }
    if (-not $cands) { throw "no scbw.exe found: build with cargo build --release, or pass -Exe" }
    $Exe = ($cands | Sort-Object { (Get-Item $_).LastWriteTime } -Descending | Select-Object -First 1)
}
$mapPath = if (Test-Path $Map) { (Resolve-Path $Map).Path } else { Join-Path $install "maps\$Map" }
if (-not (Test-Path $mapPath)) { throw "map not found: $mapPath" }

# The browser page loads the install from web\auto (gitignored, local copies).
$auto = Join-Path $repo "web\auto"
New-Item -ItemType Directory -Force $auto | Out-Null
foreach ($n in "storm.dll", "patch_rt.mpq", "broodat.mpq", "stardat.mpq") {
    $src = Get-ChildItem $install -File | Where-Object { $_.Name -ieq $n } | Select-Object -First 1
    if (-not $src) { throw "missing $n in $install" }
    if (-not (Test-Path (Join-Path $auto $n))) { Copy-Item $src.FullName (Join-Path $auto $n) }
}
if (-not (Test-Path (Join-Path $repo "web\pkg\scbw.js"))) {
    Write-Warning "web\pkg is not built: run web\build.sh (wasm-bindgen output) first"
}

$lanIp = $null
if ($Lan) {
    $env:SCBW_NET_BIND = "0.0.0.0"
    $lanIp = (Get-NetIPAddress -AddressFamily IPv4 |
        Where-Object { $_.IPAddress -notlike "127.*" -and $_.IPAddress -notlike "169.254.*" -and $_.PrefixOrigin -ne "WellKnown" } |
        Select-Object -First 1).IPAddress
}

$web = $null
try {
    if (-not $NoWeb) {
        $py = Get-Command python -ErrorAction SilentlyContinue
        $pyExe = if ($py -and $py.Source -notlike "*WindowsApps*") { $py.Source } else { Join-Path $repo "agent\local\scratch\venv\Scripts\python.exe" }
        # -WorkingDirectory instead of -d: Start-Process (PS 5.1) does not quote arguments with spaces.
        $web = Start-Process $pyExe -ArgumentList "-m", "http.server", $WebPort, "-b", "127.0.0.1" `
            -WorkingDirectory (Join-Path $repo "web") -WindowStyle Hidden -PassThru `
            -RedirectStandardOutput (Join-Path $env:TEMP "scbw_web.out") -RedirectStandardError (Join-Path $env:TEMP "scbw_web.log")
        Start-Sleep -Milliseconds 800
        if ($web.HasExited) { throw "the web server exited at once (port $WebPort in use? python: $pyExe; log: $env:TEMP\scbw_web.log)" }
        Write-Host "web page: http://localhost:$WebPort/   (server pid $($web.Id))"
    }
    Write-Host "game: $Exe"
    Write-Host "map:  $mapPath   players $Players   delay $Delay   port $Port"
    & $Exe nethost $mapPath $Port $Players $Delay | ForEach-Object {
        Write-Host $_
        if ($_ -match "^LISTEN (.+)$") {
            $code = $Matches[1]
            if ($lanIp) { $code = $code -replace "^0\.0\.0\.0", $lanIp }
            Write-Host ""
            Write-Host "JOIN CODE: $code"
            if (-not $NoWeb) {
                Write-Host "BROWSER:   http://localhost:$WebPort/?join=$($code -replace '#', '%23')"
            }
            Write-Host "(or paste the code into the join field of the page)"
            Write-Host ""
        }
    }
} finally {
    if ($web -and -not $web.HasExited) { Stop-Process -Id $web.Id -Force }
}
