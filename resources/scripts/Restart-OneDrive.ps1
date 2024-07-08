Write-Host "Restarting OneDrive..."

$processes = Get-Process -Name OneDrive -ErrorAction SilentlyContinue
$path = $null

if ($processes) {
    $path = $processes[0].Path

    Write-Host "Shutting down OneDrive..."
    & $path "/shutdown" "/background"

    foreach ($p in $processes) {
        Write-Host "Waiting for OneDrive process ($($p.Id)) to exit..."
        Wait-Process -Id $p.Id -Timeout 30 -ErrorAction SilentlyContinue
    }
}
else {
    Write-Host "OneDrive is not running"

    Write-Host "Looking for OneDrive executable..."
    foreach ($p in @(
            "${Env:LOCALAPPDATA}\Microsoft\OneDrive\OneDrive.exe",
            "${Env:ProgramFiles}\Microsoft OneDrive\OneDrive.exe",
            "${Env:ProgramFiles(x86)}\Microsoft OneDrive\OneDrive.exe"
        )) {
        if (Test-Path $p) {
            Write-Host "OneDrive executable found at $p"
            $path = $p
            break
        }
    }

    if (-not $path) {
        Write-Host "OneDrive executable not found"
        exit 1
    }
}

Write-Host "Starting OneDrive..."
Start-Process -FilePath $path -ArgumentList "/background" -NoNewWindow

Write-Host "OneDrive started"
