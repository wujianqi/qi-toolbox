# Qi Toolbox 发布构建脚本（单包：中英文内置，启动跟随系统语言，可在主面板侧栏切换）
# 用法: .\build_release.ps1
# 输出: release\qi-toolbox.exe

$ErrorActionPreference = "Stop"
$releaseDir = "release"

Write-Host "=== Qi Toolbox Release Build ===" -ForegroundColor Cyan

# 清理旧产物
if (Test-Path $releaseDir) { Remove-Item $releaseDir -Recurse -Force }
New-Item -ItemType Directory -Path $releaseDir -Force | Out-Null

# 构建
Write-Host "`n[1/1] Building..." -ForegroundColor Yellow
cargo build --release
if ($LASTEXITCODE -ne 0) { Write-Error "Build failed"; exit 1 }
Copy-Item "target\release\qi-toolbox.exe" "$releaseDir\qi-toolbox.exe"
Write-Host "  -> $releaseDir\qi-toolbox.exe" -ForegroundColor Green

# 复制发布说明
if (Test-Path "README.md") { Copy-Item "README.md" "$releaseDir\" }

# 输出摘要
$size = [math]::Round((Get-Item "$releaseDir\qi-toolbox.exe").Length / 1MB, 1)
Write-Host "`n=== Done ===" -ForegroundColor Cyan
Write-Host "  qi-toolbox.exe  ${size} MB"
Write-Host "  Output: $releaseDir/" -ForegroundColor Gray
