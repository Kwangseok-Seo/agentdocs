# Install agentdocs on Windows from its GitHub releases, in PowerShell:
#
#   irm https://github.com/Kwangseok-Seo/agentdocs/releases/latest/download/install.ps1 | iex
#
# $env:AGENTDOCS_VERSION = 'v0.1.0' installs that release rather than the
# latest; $env:AGENTDOCS_INSTALL_DIR puts the binary somewhere other than
# %LOCALAPPDATA%\Programs\agentdocs\bin; $env:AGENTDOCS_NO_MODIFY_PATH = '1'
# leaves PATH alone. Run through `iex`, the script ends with `throw` and
# never `exit`, which would close the window it runs in.

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
# Windows PowerShell 5.1 may not offer TLS 1.2, which GitHub requires.
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$repo = 'https://github.com/Kwangseok-Seo/agentdocs'
$version = if ($env:AGENTDOCS_VERSION) { $env:AGENTDOCS_VERSION } else { 'latest' }
$dir = if ($env:AGENTDOCS_INSTALL_DIR) { $env:AGENTDOCS_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\agentdocs\bin' }

# One Windows release, for x86_64; Windows on ARM runs it under emulation.
if (-not [Environment]::Is64BitOperatingSystem) { throw 'agentdocs: there is no release for 32-bit Windows' }
$target = 'x86_64-pc-windows-msvc'
$archive = "agentdocs-$target.zip"
$from = if ($version -eq 'latest') { "$repo/releases/latest/download" } else { "$repo/releases/download/$version" }

$tmp = Join-Path ([IO.Path]::GetTempPath()) ('agentdocs-' + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Write-Host "agentdocs: downloading $archive ($version)"
    Invoke-WebRequest -UseBasicParsing -Uri "$from/$archive" -OutFile (Join-Path $tmp $archive)
    Invoke-WebRequest -UseBasicParsing -Uri "$from/SHA256SUMS" -OutFile (Join-Path $tmp 'SHA256SUMS')

    # A line of SHA256SUMS is the hash and the file's name, which some tools
    # write with a `*` before it.
    $expected = Get-Content (Join-Path $tmp 'SHA256SUMS') |
        ForEach-Object { $hash, $name = $_ -split '\s+', 2; if (($name -replace '^\*', '') -eq $archive) { $hash } } |
        Select-Object -First 1
    if (-not $expected) { throw "agentdocs: SHA256SUMS has no line for $archive" }
    $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $archive)).Hash
    if ($actual -ne $expected) { throw "agentdocs: $archive does not match its checksum; nothing was installed" }

    Expand-Archive -Path (Join-Path $tmp $archive) -DestinationPath $tmp
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    Copy-Item -Force (Join-Path $tmp "agentdocs-$target\agentdocs.exe") (Join-Path $dir 'agentdocs.exe')
} finally {
    Remove-Item -Recurse -Force $tmp
}

$said = & (Join-Path $dir 'agentdocs.exe') --version
Write-Host "agentdocs: installed $said as $(Join-Path $dir 'agentdocs.exe')"

if ($env:AGENTDOCS_NO_MODIFY_PATH -eq '1') {
    Write-Host "agentdocs: PATH left as it was"
} else {
    # The user's PATH as stored — `%USERPROFILE%` and all, not expanded — and
    # written back as the kind of value it was, so that no entry of it changes.
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
    try {
        $stored = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        $entries = @($stored -split ';' | Where-Object { $_ })
        if ($entries -contains $dir) {
            $added = $false
        } else {
            $kind = if ($stored) { $key.GetValueKind('Path') } else { [Microsoft.Win32.RegistryValueKind]::ExpandString }
            $key.SetValue('Path', (($entries + $dir) -join ';'), $kind)
            $added = $true
        }
    } finally {
        $key.Close()
    }
    if ($added) {
        # Setting a variable through .NET tells running programs — Explorer,
        # and so every terminal opened after — that the environment changed.
        [Environment]::SetEnvironmentVariable('AGENTDOCS_INSTALLED', '1', 'User')
        [Environment]::SetEnvironmentVariable('AGENTDOCS_INSTALLED', $null, 'User')
        $env:Path = "$env:Path;$dir"
        Write-Host "agentdocs: added $dir to your PATH; terminals opened from now on will find agentdocs"
    }
}
