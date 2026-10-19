param([string]$Destination = "$HOME\atarustia", [switch]$NoWeb)
$ErrorActionPreference = 'Stop'
if (-not (Get-Command git -ErrorAction SilentlyContinue)) { throw 'Install Git: https://git-scm.com/download/win' }
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'Install Rust: https://rustup.rs, then reopen PowerShell.' }
if (Test-Path $Destination) { throw "Directory already exists: $Destination. No files were changed." }
git clone --origin origin 'https://github.com/qhozek/atarustia.git' $Destination
if ($LASTEXITCODE -ne 0) { throw 'git clone failed' }
Push-Location $Destination
try {
    cargo fetch --locked
    if ($LASTEXITCODE -ne 0) { throw 'cargo fetch failed' }
    if ($NoWeb) { cargo run --locked -- --no-web } else { cargo run --locked }
    if ($LASTEXITCODE -ne 0) { throw 'atarustia exited with an error' }
} finally { Pop-Location }
