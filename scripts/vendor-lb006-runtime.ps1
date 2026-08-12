param(
  [string]$RepoRoot = (Split-Path -Parent $PSScriptRoot)
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$RepoRoot = [IO.Path]::GetFullPath($RepoRoot)
$PythonVersion = '3.12.10'
$PythonArchive = "python-$PythonVersion-embed-amd64.zip"
$PythonUrl = "https://www.python.org/ftp/python/$PythonVersion/$PythonArchive"
$PythonArchiveSha256 = '4acbed6dd1c744b0376e3b1cf57ce906f9dc9e95e68824584c8099a63025a3c3'
$CodingVersion = '0.2.2'
$CodingCommit = '311c1f2529d0f047ad2a8b68db6bf92dbb93d6bc'
$CodingTree = '5ef5e638a12aa74dc8836ca02a88490c2fe019a6'
$CodingArchiveSha256 = '227b94128eaacda8d63d391911db1918e0bd813718d9ea5d276b5aab7eac73fd'
$PyJwtVersion = '2.10.1'
$PyJwtWheel = 'PyJWT-2.10.1-py3-none-any.whl'
$PyJwtWheelSha256 = 'dcdd193e30abefd5debf142f9adfcdd2b58004e644f25406ffaebd50bd98dacb'

function Get-Sha256([string]$Path) {
  return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Assert-Sha256([string]$Path, [string]$Expected, [string]$Label) {
  $actual = Get-Sha256 $Path
  if ($actual -ne $Expected) {
    throw "$Label SHA256 mismatch: $actual"
  }
}

function Get-TreeSha256([string]$Root, [string[]]$ExcludeNames = @()) {
  $rootFull = [IO.Path]::GetFullPath($Root).TrimEnd('\')
  $lines = @(Get-ChildItem -LiteralPath $rootFull -Recurse -File |
    Where-Object { $ExcludeNames -notcontains $_.Name } |
    ForEach-Object {
      $relative = $_.FullName.Substring($rootFull.Length).TrimStart('\').Replace('\','/')
      $hash = Get-Sha256 $_.FullName
      "$relative`0$hash"
    })
  [Array]::Sort($lines, [StringComparer]::Ordinal)
  $canonical = [string]::Join("`n", $lines)
  $bytes = [Text.Encoding]::UTF8.GetBytes($canonical)
  $sha = [Security.Cryptography.SHA256]::Create()
  try {
    return ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-','').ToLowerInvariant()
  } finally {
    $sha.Dispose()
  }
}

function Write-JsonUtf8([string]$Path, [object]$Value) {
  $json = $Value | ConvertTo-Json -Depth 8
  [IO.File]::WriteAllText($Path, $json + "`n", [Text.UTF8Encoding]::new($false))
}

$pythonRoot = Join-Path $RepoRoot 'runtime\python'
$codingRoot = Join-Path $RepoRoot 'runtime\coding-tools-mcp'
$sitePackages = Join-Path $codingRoot 'site-packages'
$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ('localbridge-lb006-vendor-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null

try {
  $pythonZip = Join-Path $tempRoot $PythonArchive
  Invoke-WebRequest -UseBasicParsing -Uri $PythonUrl -OutFile $pythonZip
  Assert-Sha256 $pythonZip $PythonArchiveSha256 'Python embeddable archive'

  $sourceRepo = Join-Path $tempRoot 'coding-tools-mcp.git'
  git -c core.longpaths=true clone --quiet --no-checkout --filter=blob:none https://github.com/xyTom/coding-tools-mcp.git $sourceRepo
  if ($LASTEXITCODE -ne 0) { throw 'coding-tools-mcp clone failed' }
  $resolvedCommit = (git -C $sourceRepo rev-parse $CodingCommit).Trim()
  $resolvedTree = (git -C $sourceRepo rev-parse "$CodingCommit^{tree}").Trim()
  if ($resolvedCommit -ne $CodingCommit) { throw "coding-tools commit mismatch: $resolvedCommit" }
  if ($resolvedTree -ne $CodingTree) { throw "coding-tools tree mismatch: $resolvedTree" }

  $fullArchive = Join-Path $tempRoot 'coding-tools-mcp-full.tar'
  git -C $sourceRepo archive --format=tar --output=$fullArchive $CodingCommit
  if ($LASTEXITCODE -ne 0) { throw 'coding-tools full git archive failed' }
  Assert-Sha256 $fullArchive $CodingArchiveSha256 'coding-tools full git archive'

  $subsetArchive = Join-Path $tempRoot 'coding-tools-mcp-runtime.tar'
  git -C $sourceRepo archive --format=tar --output=$subsetArchive $CodingCommit coding_tools_mcp LICENSE pyproject.toml README.md
  if ($LASTEXITCODE -ne 0) { throw 'coding-tools runtime git archive failed' }

  $wheelDir = Join-Path $tempRoot 'wheel'
  New-Item -ItemType Directory -Force -Path $wheelDir | Out-Null
  py -3.13 -m pip download --disable-pip-version-check --no-deps --only-binary=:all: --dest $wheelDir "PyJWT==$PyJwtVersion"
  if ($LASTEXITCODE -ne 0) { throw 'build-time PyJWT wheel download failed; no product runtime fallback is permitted' }
  $wheelPath = Join-Path $wheelDir $PyJwtWheel
  Assert-Sha256 $wheelPath $PyJwtWheelSha256 'PyJWT wheel'

  foreach ($root in @($pythonRoot, $codingRoot)) {
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
    New-Item -ItemType Directory -Force -Path $root | Out-Null
  }

  Expand-Archive -LiteralPath $pythonZip -DestinationPath $pythonRoot -Force
  tar -xf $subsetArchive -C $codingRoot
  if ($LASTEXITCODE -ne 0) { throw 'coding-tools runtime source extraction failed' }
  New-Item -ItemType Directory -Force -Path $sitePackages | Out-Null

  Add-Type -AssemblyName System.IO.Compression.FileSystem
  [IO.Compression.ZipFile]::ExtractToDirectory($wheelPath, $sitePackages)

  $pth = Join-Path $pythonRoot 'python312._pth'
  @(
    'python312.zip',
    '.',
    '..\coding-tools-mcp',
    '..\coding-tools-mcp\site-packages',
    'import site'
  ) | Set-Content -LiteralPath $pth -Encoding ascii

  $pipPayload = Get-ChildItem -LiteralPath $pythonRoot,$codingRoot -Recurse -Directory -Force |
    Where-Object {
      $_.Name -match '^(pip|pip-[0-9].*\.dist-info|setuptools|setuptools-[0-9].*\.dist-info|wheel|wheel-[0-9].*\.dist-info)$'
    }
  if ($pipPayload) {
    throw ('Runtime unexpectedly contains installer payload: ' + (($pipPayload | ForEach-Object FullName) -join '; '))
  }

  $pythonExe = Join-Path $pythonRoot 'python.exe'
  $probe = & $pythonExe -I -B -c "import sys,coding_tools_mcp,jwt; print(sys.version.split()[0]); print(coding_tools_mcp.__version__); print(jwt.__version__); print(int(sys.flags.isolated)); print(int(sys.flags.no_user_site))"
  if ($LASTEXITCODE -ne 0) { throw 'bundled embedded Python import probe failed' }
  if ($probe.Count -lt 5 -or $probe[0].Trim() -ne $PythonVersion -or $probe[1].Trim() -ne $CodingVersion -or $probe[2].Trim() -ne $PyJwtVersion -or $probe[3].Trim() -ne '1' -or $probe[4].Trim() -ne '1') {
    throw ('bundled embedded Python probe returned unexpected values: ' + ($probe -join '|'))
  }

  $nondeterministicPayload = Get-ChildItem -LiteralPath $pythonRoot,$codingRoot -Recurse -Force |
    Where-Object {
      $_.Name -eq '__pycache__' -or $_.Name -match '\.pyc$' -or $_.Name -eq 'direct_url.json'
    }
  if ($nondeterministicPayload) {
    throw ('Runtime unexpectedly contains nondeterministic generated payload: ' + (($nondeterministicPayload | ForEach-Object FullName) -join '; '))
  }

  $pythonMetaPath = Join-Path $pythonRoot 'runtime-metadata.json'
  $codingMetaPath = Join-Path $codingRoot 'runtime-metadata.json'
  $pythonMetadata = [ordered]@{
    schema_version = 1
    runtime = 'python-embedded'
    version = $PythonVersion
    source = $PythonUrl
    source_archive = $PythonArchive
    source_archive_sha256 = $PythonArchiveSha256
    executable = 'runtime/python/python.exe'
    executable_sha256 = Get-Sha256 $pythonExe
    python312_dll_sha256 = Get-Sha256 (Join-Path $pythonRoot 'python312.dll')
    stdlib_zip_sha256 = Get-Sha256 (Join-Path $pythonRoot 'python312.zip')
    pth_sha256 = Get-Sha256 $pth
    isolated = $true
    user_site_enabled = $false
    runtime_pip_present = $false
    external_python_fallback = $false
  }
  Write-JsonUtf8 $pythonMetaPath $pythonMetadata
  $pythonMetadata['payload_tree_sha256'] = Get-TreeSha256 $pythonRoot @('runtime-metadata.json')
  Write-JsonUtf8 $pythonMetaPath $pythonMetadata

  $codingMetadata = [ordered]@{
    schema_version = 1
    runtime = 'coding-tools-mcp'
    version = $CodingVersion
    source = 'https://github.com/xyTom/coding-tools-mcp'
    git_commit = $CodingCommit
    git_tree = $CodingTree
    full_git_archive_sha256 = $CodingArchiveSha256
    runtime_subset_archive_sha256 = Get-Sha256 $subsetArchive
    entry_module = 'coding_tools_mcp'
    dependency_pyjwt_version = $PyJwtVersion
    dependency_pyjwt_wheel = $PyJwtWheel
    dependency_pyjwt_wheel_sha256 = $PyJwtWheelSha256
    runtime_pip_present = $false
  }
  Write-JsonUtf8 $codingMetaPath $codingMetadata
  $codingMetadata['payload_tree_sha256'] = Get-TreeSha256 $codingRoot @('runtime-metadata.json')
  Write-JsonUtf8 $codingMetaPath $codingMetadata

  Write-Output ('LB006_VENDOR=PASS python=' + $PythonVersion + ' coding_tools=' + $CodingVersion + ' pyjwt=' + $PyJwtVersion)
  Write-Output ('PYTHON_EXE_SHA256=' + $pythonMetadata.executable_sha256)
  Write-Output ('PYTHON_PAYLOAD_TREE_SHA256=' + $pythonMetadata.payload_tree_sha256)
  Write-Output ('CODING_PAYLOAD_TREE_SHA256=' + $codingMetadata.payload_tree_sha256)
  Write-Output ('CODING_RUNTIME_SUBSET_SHA256=' + $codingMetadata.runtime_subset_archive_sha256)
} finally {
  if (Test-Path -LiteralPath $tempRoot) { Remove-Item -LiteralPath $tempRoot -Recurse -Force }
}
