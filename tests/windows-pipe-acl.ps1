Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$originalRuntimeDir = $env:AGENT_EDITOR_RUNTIME_DIR
$tempRoot = Join-Path ($env:RUNNER_TEMP ?? $env:TEMP) ("spiki-pipe-acl-" + [guid]::NewGuid().ToString("N"))
$runtimeDir = Join-Path $tempRoot "runtime"
$probePath = Join-Path $tempRoot "probe.ps1"
$stdoutPath = Join-Path $tempRoot "probe.stdout.txt"
$stderrPath = Join-Path $tempRoot "probe.stderr.txt"
$userName = "spiki_acl_" + [guid]::NewGuid().ToString("N").Substring(0, 8)
$passwordPlain = "Spiki!Acl#2026"
$securePassword = ConvertTo-SecureString $passwordPlain -AsPlainText -Force
$userCreated = $false

New-Item -ItemType Directory -Force -Path $runtimeDir | Out-Null
$env:AGENT_EDITOR_RUNTIME_DIR = $runtimeDir

try {
    node --input-type=module -e 'import { ensureDaemonRunning } from "./launcher/daemon-bootstrap.mjs"; await ensureDaemonRunning();' | Out-Null
    $status = node ./bin/spiki.js daemon status | ConvertFrom-Json
    if (-not $status.reachable) {
        throw "spiki daemon is not reachable on Windows host smoke"
    }
    if (-not $status.compatible) {
        throw "spiki daemon is reachable but not compatible on Windows host smoke"
    }
    if (-not $status.socketPath.StartsWith("\\.\pipe\")) {
        throw "expected a Windows named pipe path but received: $($status.socketPath)"
    }

    net user $userName $passwordPlain /add /y | Out-Null
    $userCreated = $true

    $pipeName = $status.socketPath.Substring("\\.\pipe\".Length)
    @"
try {
    \$client = [System.IO.Pipes.NamedPipeClientStream]::new('.', '$pipeName', [System.IO.Pipes.PipeDirection]::InOut)
    \$client.Connect(1500)
    Write-Output 'connected'
    exit 0
} catch {
    Write-Output \$_.Exception.GetType().FullName
    Write-Output \$_.Exception.Message
    exit 23
}
"@ | Set-Content -Path $probePath -Encoding UTF8

    $credential = [System.Management.Automation.PSCredential]::new(".\$userName", $securePassword)
    $process = Start-Process powershell `
        -ArgumentList @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $probePath) `
        -Credential $credential `
        -Wait `
        -PassThru `
        -RedirectStandardOutput $stdoutPath `
        -RedirectStandardError $stderrPath

    $combinedOutput = @(
        (Get-Content $stdoutPath -Raw -ErrorAction SilentlyContinue),
        (Get-Content $stderrPath -Raw -ErrorAction SilentlyContinue)
    ) -join "`n"

    if ($process.ExitCode -eq 0) {
        throw "secondary Windows user unexpectedly connected to $pipeName`n$combinedOutput"
    }
    if ($process.ExitCode -ne 23) {
        throw "secondary Windows user probe failed unexpectedly with exit code $($process.ExitCode)`n$combinedOutput"
    }
    if ($combinedOutput -notmatch "UnauthorizedAccessException|access is denied|denied") {
        throw "secondary Windows user did not fail with an access-denied signal`n$combinedOutput"
    }
} finally {
    try {
        node ./bin/spiki.js daemon stop | Out-Null
    } catch {}
    if ($userCreated) {
        try {
            net user $userName /delete | Out-Null
        } catch {}
    }
    if ($null -eq $originalRuntimeDir) {
        Remove-Item Env:AGENT_EDITOR_RUNTIME_DIR -ErrorAction SilentlyContinue
    } else {
        $env:AGENT_EDITOR_RUNTIME_DIR = $originalRuntimeDir
    }
    Remove-Item -Recurse -Force $tempRoot -ErrorAction SilentlyContinue
}
