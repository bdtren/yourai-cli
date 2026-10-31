# Stops this YourAI project's server. Requires the `yourai` CLI on PATH. Does not delete anything.
$ErrorActionPreference = 'Stop'
Push-Location (Join-Path $PSScriptRoot '..')
try { & yourai stop @args; $code = $LASTEXITCODE } finally { Pop-Location }
if ($code) { Write-Error "yourai stop failed with exit code $code" }
