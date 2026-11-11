# Ядро Xray, базы geoip/geosite и драйвер Wintun для сборки под Windows.
#
# В репозиторий они не входят: это чужие бинарники. Версии и контрольные
# суммы зафиксированы здесь. Скачанное сверяется с суммой, и при
# несовпадении сборка останавливается: подменённое ядро в установщик
# попасть не должно.
$ErrorActionPreference = 'Stop'

$XrayUrl    = 'https://github.com/XTLS/Xray-core/releases/download/v26.11.9/Xray-windows-64.zip'
$XraySha    = '1262522266337b824f07beb8e1375786b437683b3890105469088f8e17e0a69d'
$WintunUrl  = 'https://www.wintun.net/builds/wintun-0.14.1.zip'
$WintunSha  = '07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51'

$root = Split-Path -Parent $PSScriptRoot
$dst  = Join-Path $root 'src-tauri\resources'
$tmp  = Join-Path $env:TEMP ("cryvpn-res-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $dst, $tmp | Out-Null

function Get-Verified($url, $sha, $out) {
  Invoke-WebRequest -Uri $url -OutFile $out -TimeoutSec 300
  $got = (Get-FileHash -Algorithm SHA256 $out).Hash.ToLower()
  if ($got -ne $sha) { throw "Checksum mismatch for $url`n expected $sha`n got      $got" }
}

Get-Verified $XrayUrl $XraySha (Join-Path $tmp 'xray.zip')
Expand-Archive -LiteralPath (Join-Path $tmp 'xray.zip') -DestinationPath (Join-Path $tmp 'xray') -Force
Copy-Item (Join-Path $tmp 'xray\xray.exe'), (Join-Path $tmp 'xray\geoip.dat'), (Join-Path $tmp 'xray\geosite.dat') $dst -Force

Get-Verified $WintunUrl $WintunSha (Join-Path $tmp 'wintun.zip')
Expand-Archive -LiteralPath (Join-Path $tmp 'wintun.zip') -DestinationPath (Join-Path $tmp 'wintun') -Force
Copy-Item (Join-Path $tmp 'wintun\wintun\bin\amd64\wintun.dll') $dst -Force

Remove-Item $tmp -Recurse -Force
Get-ChildItem $dst | ForEach-Object { $_.Name + ' - ' + [math]::Round($_.Length/1MB,2) + ' MB' }
