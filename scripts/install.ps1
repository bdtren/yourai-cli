# yourai installer: irm https://yourai.bdtren.com/cli/install.ps1 | iex
# Options via environment variables:
#   YOURAI_VERSION, YOURAI_BASE_URL, YOURAI_INSTALL_DIR, YOURAI_GITHUB_REPO,
#   YOURAI_NO_MODIFY_PATH=1, YOURAI_DRY_RUN=1
# Installs into your user profile. Never needs administrator rights.

function Install-YourAI {
    $ErrorActionPreference = 'Stop'
    $oldProgress = $ProgressPreference
    $ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest is very slow with the progress bar
    $tmp = $null
    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

        $baseUrl    = if ($env:YOURAI_BASE_URL)    { $env:YOURAI_BASE_URL }    else { 'https://yourai.bdtren.com/cli' }
        $installDir = if ($env:YOURAI_INSTALL_DIR) { $env:YOURAI_INSTALL_DIR } else { Join-Path $env:USERPROFILE '.yourai\bin' }
        $githubRepo = if ($env:YOURAI_GITHUB_REPO) { $env:YOURAI_GITHUB_REPO } else { 'bdtren/yourai-cli' }

        $procArch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
        $arch = switch ($procArch) {
            'AMD64' { 'x86_64' }
            'ARM64' { 'aarch64' }
            default { throw "Unsupported CPU architecture: $procArch" }
        }
        $target = "$arch-pc-windows-msvc"

        $version = $env:YOURAI_VERSION
        if (-not $version) { $version = (Invoke-RestMethod "$baseUrl/latest.txt").Trim() }
        if (-not $version.StartsWith('v')) { $version = "v$version" }

        $archive = "yourai-$version-$target.zip"
        $url = "$baseUrl/releases/$version/$archive"

        if ($env:YOURAI_DRY_RUN) {
            Write-Host "yourai: dry run: would install $version for $target"
            Write-Host "yourai:   download: $url"
            Write-Host "yourai:   install:  $installDir\yourai.exe"
            return
        }

        $tmp = Join-Path ([IO.Path]::GetTempPath()) ("yourai-" + [Guid]::NewGuid())
        New-Item -ItemType Directory -Path $tmp | Out-Null

        Write-Host "yourai: installing $version for $target"
        $zipPath = Join-Path $tmp $archive
        Invoke-WebRequest $url -OutFile $zipPath -UseBasicParsing
        $expected = ((Invoke-RestMethod "$url.sha256").Trim() -split '\s+')[0].ToLower()
        $actual   = (Get-FileHash $zipPath -Algorithm SHA256).Hash.ToLower()
        if ($expected -ne $actual) { throw "Checksum mismatch. Nothing was installed." }

        try {
            $ghUrl = "https://github.com/$githubRepo/releases/download/$version/$archive.sha256"
            $second = ((Invoke-RestMethod $ghUrl -TimeoutSec 15).Trim() -split '\s+')[0].ToLower()
            if ($second -ne $actual) { throw "Checksum differs from the GitHub release. Nothing was installed." }
            Write-Host "yourai: checksum verified against $baseUrl and GitHub"
        } catch {
            if ($_.Exception.Message -like 'Checksum differs*') { throw }
            Write-Host "yourai: warning: could not reach GitHub to double-check the checksum" -ForegroundColor Yellow
        }

        Expand-Archive $zipPath -DestinationPath $tmp -Force
        $newExe = Join-Path $tmp 'yourai.exe'
        if (-not (Test-Path $newExe)) { throw "The archive does not contain yourai.exe" }
        New-Item -ItemType Directory -Path $installDir -Force | Out-Null

        # A running .exe cannot be overwritten on Windows, but it can be renamed.
        $dest = Join-Path $installDir 'yourai.exe'
        if (Test-Path $dest) { Move-Item $dest "$dest.old" -Force }
        Copy-Item $newExe $dest -Force
        Remove-Item "$dest.old" -Force -ErrorAction SilentlyContinue

        $home2 = Join-Path $env:USERPROFILE '.yourai'
        New-Item -ItemType Directory -Path $home2 -Force | Out-Null
        $receipt = @{ method = 'script'; version = $version; install_dir = $installDir } | ConvertTo-Json -Compress
        Set-Content -Path (Join-Path $home2 'install-receipt.json') -Value $receipt -Encoding UTF8

        if (-not $env:YOURAI_NO_MODIFY_PATH) {
            $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
            if (-not $userPath) { $userPath = '' }
            if (($userPath -split ';') -notcontains $installDir) {
                $newPath = (($userPath.TrimEnd(';')), $installDir) -join ';'
                [Environment]::SetEnvironmentVariable('Path', $newPath.TrimStart(';'), 'User')
                $env:Path += ";$installDir"
                Write-Host "yourai: added $installDir to your user PATH"
            }
        }
        Write-Host "yourai: installed to $dest"
        Write-Host "yourai: next, run  yourai doctor  then  yourai init"
    }
    catch {
        Write-Host "yourai: error: $($_.Exception.Message)" -ForegroundColor Red
    }
    finally {
        $ProgressPreference = $oldProgress
        if ($tmp -and (Test-Path $tmp)) { Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue }
    }
}

Install-YourAI
