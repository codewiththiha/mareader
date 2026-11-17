# Usage: tools/exe-arch.ps1 PATH.exe EXPECTED (x64 | arm64)
# Fails unless the PE binary targets the expected Windows architecture.
param(
    [Parameter(Mandatory)][string]$Exe,
    [Parameter(Mandatory)][ValidateSet('x64', 'arm64')][string]$Expected
)
$ErrorActionPreference = 'Stop'
$bytes = [System.IO.File]::ReadAllBytes((Resolve-Path $Exe).Path)
if ($bytes.Length -lt 64 -or $bytes[0] -ne 0x4D -or $bytes[1] -ne 0x5A) {
    Write-Error "$Exe is not a PE executable (missing MZ header)"
    exit 1
}
$peOffset = [BitConverter]::ToInt32($bytes, 0x3C)
if ($peOffset -lt 0 -or $peOffset + 6 -gt $bytes.Length) {
    Write-Error "$Exe has an invalid PE header offset ($peOffset)"
    exit 1
}
$sig = [Text.Encoding]::ASCII.GetString($bytes, $peOffset, 4)
if ($sig -ne "PE`0`0") {
    Write-Error "$Exe is missing the PE signature"
    exit 1
}
$machine = [BitConverter]::ToUInt16($bytes, $peOffset + 4)
$actual = switch ($machine) {
    0x8664 { 'x64' }
    0xAA64 { 'arm64' }
    0x014C { 'x86' }
    default { '0x{0:X4}' -f $machine }
}
if ($actual -ne $Expected) {
    Write-Error "$Exe is $actual, expected $Expected"
    exit 1
}
Write-Output "$Exe targets $actual"
