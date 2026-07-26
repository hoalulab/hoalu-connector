#Requires -Version 5.1
<#
  Build the complete plugin: Rust staticlib (x86 + x64) -> C++ shim DLL.
  Run from "Developer PowerShell for VS" (the MSVC toolchain must be in PATH).
 
  Usage:
    .\scripts\build.ps1                # build x86 and x64 in Release mode
    .\scripts\build.ps1 -Arch x64       # build x64 only
#>
 
param(
    [ValidateSet("x86", "x64", "both")]
    [string]$Arch = "both"
)
 
$ErrorActionPreference = "Stop"

# Some launchers inject both `Path` and `PATH`. MSBuild 18 copies the process
# environment into a case-insensitive dictionary and fails when both exist.
# Recreate a single canonical entry for this build process.
$buildProcessPath = $env:Path
Remove-Item Env:PATH -ErrorAction SilentlyContinue
$env:Path = $buildProcessPath
 
# PowerShell 7.3+ may treat all native stderr output (including harmless rustup
# "info:" messages) as terminating errors when $ErrorActionPreference is
# "Stop". Disable that behavior and check exit codes explicitly.
if (Test-Path variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}
 
function Invoke-Native {
    param([string]$Description, [scriptblock]$Command)
    Write-Host "==> $Description" -ForegroundColor Cyan

    # Windows PowerShell 5 wraps native stderr as ErrorRecord objects. Tools such
    # as rustup and cargo legitimately write progress messages to stderr, so
    # temporarily avoid promoting those messages to terminating errors. The
    # native process exit code remains the authoritative success signal.
    $previousErrorActionPreference = $ErrorActionPreference
    $nativeExitCode = $null
    try {
        $ErrorActionPreference = "Continue"
        & $Command
        $nativeExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previousErrorActionPreference
    }

    if ($nativeExitCode -ne 0) {
        throw "$Description that bai (exit code $nativeExitCode)"
    }
}
 
$root = Split-Path -Parent $PSScriptRoot
$rustCore = Join-Path $root "rust-core"
 
function Build-One {
    param([string]$RustTriple, [string]$CMakeArch)
 
    Push-Location $rustCore
    try {
        Invoke-Native "rustup target add $RustTriple" { rustup target add $RustTriple }
        Invoke-Native "cargo build --release ($RustTriple)" { cargo build --release --target $RustTriple }
    } finally {
        Pop-Location
    }
 
    $buildDir = Join-Path $root "build-$RustTriple"
    $tripleArg = "-DRUST_TARGET_TRIPLE=$RustTriple"
    Invoke-Native "cmake configure ($RustTriple)" {
        cmake -S $root -B $buildDir -A $CMakeArch $tripleArg
    }
    Invoke-Native "cmake build ($RustTriple)" {
        cmake --build $buildDir --config Release
    }
 
    $outDll = Get-ChildItem -Path $buildDir -Recurse -Filter "AmiDataPlugin.dll" | Select-Object -First 1
    if ($outDll) {
        Write-Host "==> OK: $($outDll.FullName)" -ForegroundColor Green
    } else {
        throw "Khong tim thay AmiDataPlugin.dll sau khi build ($RustTriple)"
    }
}
 
if ($Arch -eq "x86" -or $Arch -eq "both") {
    Build-One -RustTriple "i686-pc-windows-msvc" -CMakeArch "Win32"
}
if ($Arch -eq "x64" -or $Arch -eq "both") {
    Build-One -RustTriple "x86_64-pc-windows-msvc" -CMakeArch "x64"
}
 