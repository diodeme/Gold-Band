$ErrorActionPreference = 'Stop'
$captureRoot = $PSScriptRoot
$collectorPath = Join-Path $captureRoot 'otelcol-contrib.exe'
$captureConfig = Get-Content (Join-Path $captureRoot 'config.json') -Raw | ConvertFrom-Json
$capturePort = ([Uri]$captureConfig.telemetryEndpoint).Port
$captureMutex = [Threading.Mutex]::new($false, 'Local\GoldBandClaudeCaptureCollector')
$captureLocked = $false
try {
    try { $captureLocked = $captureMutex.WaitOne(30000) } catch [Threading.AbandonedMutexException] { $captureLocked = $true }
    if (-not $captureLocked) { throw 'Collector startup lock timed out' }
    $listener = Get-NetTCPConnection -LocalPort $capturePort -State Listen -ErrorAction SilentlyContinue
    if ($listener) {
        $owner = Get-Process -Id $listener[0].OwningProcess -ErrorAction Stop
        if ($owner.Path -ne $collectorPath) { throw 'Collector port belongs to a different process' }
        $health = Invoke-WebRequest $captureConfig.collectorHealth -TimeoutSec 3 -UseBasicParsing
        if ($health.StatusCode -ne 200) { throw 'Collector health check failed' }
        exit 0
    }
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss-ffff'
    $serviceLogs = Join-Path $captureRoot 'collector-service'
    New-Item -ItemType Directory -Path $serviceLogs -Force | Out-Null
    $collectorProcess = Start-Process -FilePath $collectorPath -ArgumentList @('--config', (Join-Path $captureRoot 'collector.yaml')) -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $serviceLogs "$stamp.stdout.log") -RedirectStandardError (Join-Path $serviceLogs "$stamp.stderr.log")
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        if ($collectorProcess.HasExited) { throw 'Collector exited during startup; inspect collector-service logs' }
        try {
            $health = Invoke-WebRequest $captureConfig.collectorHealth -TimeoutSec 1 -UseBasicParsing
            if ($health.StatusCode -eq 200) { exit 0 }
        } catch { }
        Start-Sleep -Milliseconds 250
    }
    throw 'Collector did not become ready'
} finally {
    if ($captureLocked) { $captureMutex.ReleaseMutex() }
    $captureMutex.Dispose()
}
