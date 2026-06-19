# Qi Toolbox 多语言发布构建脚本
# 用法: .\build_release.ps1
# 输出: release/qi-toolbox_cn.exe, release/qi-toolbox_en.exe

$ErrorActionPreference = "Stop"
$releaseDir = "release"

Write-Host "=== Qi Toolbox Release Build ===" -ForegroundColor Cyan

# 清理旧产物
if (Test-Path $releaseDir) { Remove-Item $releaseDir -Recurse -Force }
New-Item -ItemType Directory -Path $releaseDir | Out-Null

# 构建中文版 (默认)
Write-Host "`n[1/2] Building Chinese version..." -ForegroundColor Yellow
cargo build --release
if ($LASTEXITCODE -ne 0) { Write-Error "Chinese build failed"; exit 1 }
Copy-Item "target\release\qi-toolbox.exe" "$releaseDir\qi-toolbox_cn.exe"
Write-Host "  -> $releaseDir\qi-toolbox_cn.exe" -ForegroundColor Green

# 构建英文版
Write-Host "`n[2/2] Building English version..." -ForegroundColor Yellow
cargo build --release --features english
if ($LASTEXITCODE -ne 0) { Write-Error "English build failed"; exit 1 }
Copy-Item "target\release\qi-toolbox.exe" "$releaseDir\qi-toolbox_en.exe"
Write-Host "  -> $releaseDir\qi-toolbox_en.exe" -ForegroundColor Green

# 复制发布说明
if (Test-Path "README.md") { Copy-Item "README.md" "$releaseDir\" }

# 输出摘要
$cnSize = [math]::Round((Get-Item "$releaseDir\qi-toolbox_cn.exe").Length / 1MB, 1)
$enSize = [math]::Round((Get-Item "$releaseDir\qi-toolbox_en.exe").Length / 1MB, 1)
Write-Host "`n=== Done ===" -ForegroundColor Cyan
Write-Host "  qi-toolbox_cn.exe  ${cnSize} MB"
Write-Host "  qi-toolbox_en.exe  ${enSize} MB"
Write-Host "  Output: $releaseDir/" -ForegroundColor Gray
