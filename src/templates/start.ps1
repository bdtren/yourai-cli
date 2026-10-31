# Starts this YourAI project in the background. Requires the `yourai` CLI on PATH.
# Side effects: launches the local AI server and writes logs to .yourai\server.log.
$ErrorActionPreference = 'Stop'
Push-Location (Join-Path $PSScriptRoot '..')
try { & yourai start @args; $code = $LASTEXITCODE } finally { Pop-Location }
if ($code) { Write-Error "yourai start failed with exit code $code" }
