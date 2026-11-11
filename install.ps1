# "Ante" installer for Windows (PowerShell).
#
#   irm https://raw.githubusercontent.com/darkwebber/Ante/main/install.ps1 | iex
#
# Downloads a ready-made antec.exe from GitHub Releases, checks its SHA-256,
# puts it in %USERPROFILE%\.ante\bin and adds that to your PATH.
# Rust avasaram ledhu.
#
# Settings (environment variables):
#   ANTE_VERSION=v0.1.0       this release instead of the latest
#   ANTE_HOME                 where to install (antec.exe goes in $ANTE_HOME\bin)
#   ANTE_NO_MODIFY_PATH=1     don't change PATH
#   ANTE_DIST=<url>           download from a mirror (same file names)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Repo = 'darkwebber/Ante'
$AnteHome = if ($env:ANTE_HOME) { $env:ANTE_HOME } else { Join-Path $env:USERPROFILE '.ante' }
$Bin = Join-Path $AnteHome 'bin'

function Fail($msg) {
    Write-Host "error: $msg" -ForegroundColor Red
    exit 1
}

# Only an x64 build is published; Windows on ARM runs it emulated.
$target = 'x86_64-pc-windows-msvc'

if ($env:ANTE_VERSION) {
    $tag = $env:ANTE_VERSION
} elseif ($env:ANTE_DIST) {
    Fail 'ANTE_DIST tho paatu ANTE_VERSION kuda ivvandi'
} else {
    try {
        $release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ 'User-Agent' = 'ante-installer' }
        $tag = $release.tag_name
    } catch {
        Fail 'GitHub ki connect avvaledhu. Internet check chesi malli try cheyyandi'
    }
}

$name = "antec-$tag-$target"
$dist = if ($env:ANTE_DIST) { $env:ANTE_DIST } else { "https://github.com/$Repo/releases/download/$tag" }
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("ante-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null

try {
    Write-Host "📦 `"Ante`" $tag ($target) download chesthunnam..."
    try {
        Invoke-WebRequest "$dist/$name.zip" -OutFile (Join-Path $tmp "$name.zip")
        Invoke-WebRequest "$dist/$name.zip.sha256" -OutFile (Join-Path $tmp "$name.zip.sha256")
    } catch {
        Fail "$dist/$name.zip download avvaledhu"
    }
    $want = ((Get-Content (Join-Path $tmp "$name.zip.sha256") -Raw).Trim() -split '\s+')[0].ToLower()
    $got = (Get-FileHash (Join-Path $tmp "$name.zip") -Algorithm SHA256).Hash.ToLower()
    if ($want -ne $got) { Fail 'download paadayindhi (SHA-256 match avvaledhu). Malli try cheyyandi' }

    Expand-Archive (Join-Path $tmp "$name.zip") -DestinationPath $tmp -Force
    New-Item -ItemType Directory -Path $Bin -Force | Out-Null
    Copy-Item (Join-Path $tmp "$name\antec.exe") (Join-Path $Bin 'antec.exe') -Force
} finally {
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host "✅ antec ready: $Bin\antec.exe"

if (-not $env:ANTE_NO_MODIFY_PATH) {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not (($userPath -split ';') -contains $Bin)) {
        $newPath = if ($userPath) { "$Bin;$userPath" } else { $Bin }
        [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
        Write-Host "🛣️  PATH lo add chesam. Kottha terminal open cheyyandi"
    }
    $env:Path = "$Bin;$env:Path"
}

if (-not (Get-Command clang -ErrorAction SilentlyContinue)) {
    Write-Host ''
    Write-Host 'ℹ️  clang ledhu: `antec run` interpreter lo run chesthundhi, adhi ok.'
    Write-Host '   Native binaries (`antec build`) kosam LLVM install cheyyandi: winget install LLVM.LLVM'
}

Write-Host ''
& (Join-Path $Bin 'antec.exe') --version
Write-Host ''
Write-Host 'Next:'
Write-Host '  antec new naa_project; cd naa_project; antec run'
Write-Host '  antec repl        (okko line try cheyyadaniki)'
