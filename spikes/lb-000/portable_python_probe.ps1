param(
  [Parameter(Mandatory=$true)][string]$OutputPath
)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path ([IO.Path]::GetDirectoryName($OutputPath)) 'python-embed-poc'))
$download = Join-Path $root 'python-3.12.10-embed-amd64.zip'
$runtime = Join-Path $root 'runtime'
$site = Join-Path $runtime 'Lib\site-packages'
New-Item -ItemType Directory -Force -Path $root | Out-Null
if (!(Test-Path $download)) {
  Invoke-WebRequest -UseBasicParsing -Uri 'https://www.python.org/ftp/python/3.12.10/python-3.12.10-embed-amd64.zip' -OutFile $download
}
$zipHash = (Get-FileHash -Algorithm SHA256 $download).Hash.ToLowerInvariant()
$expectedZipHash = '4acbed6dd1c744b0376e3b1cf57ce906f9dc9e95e68824584c8099a63025a3c3'
if ($zipHash -ne $expectedZipHash) { throw "Python embeddable archive SHA256 mismatch: $zipHash" }
if (Test-Path $runtime) { Remove-Item -Recurse -Force $runtime }
Expand-Archive -Path $download -DestinationPath $runtime -Force
New-Item -ItemType Directory -Force -Path $site | Out-Null
$pth = Join-Path $runtime 'python312._pth'
$lines = Get-Content $pth
$normalized = @()
foreach ($line in $lines) {
  if ($line -eq '#import site') { $normalized += 'import site' } else { $normalized += $line }
}
if ($normalized -notcontains 'Lib\site-packages') { $normalized += 'Lib\site-packages' }
$normalized | Set-Content -Path $pth -Encoding ascii
$codingToolsSource = 'git+https://github.com/xyTom/coding-tools-mcp.git@311c1f2529d0f047ad2a8b68db6bf92dbb93d6bc'
& py -3.13 -m pip install --disable-pip-version-check --quiet --no-compile --upgrade --target $site $codingToolsSource
if ($LASTEXITCODE -ne 0) { throw 'build-time dependency staging failed' }
$python = Join-Path $runtime 'python.exe'
$version = (& $python -c "import sys; print(sys.version.split()[0])").Trim()
$module = (& $python -c "import coding_tools_mcp,jwt; print(coding_tools_mcp.__version__); print(jwt.__version__)")
$help = (& $python -m coding_tools_mcp --help 2>&1 | Out-String)
$probeDir = Join-Path $root 'mcp-runtime-evidence'
& $python ([IO.Path]::GetFullPath('spikes/lb-000/mcp_runtime_probe.py')) --output-dir $probeDir
$probeExit = $LASTEXITCODE
$result = [ordered]@{
  schema_version = 1
  purpose = 'LB-000 feasibility only; exact 3.12.x patch/layout remains LB-006 authority'
  sample_python_version = $version
  source = 'python.org Windows embeddable package'
  archive = 'python-3.12.10-embed-amd64.zip'
  archive_sha256 = $zipHash
  runtime_executable = $python
  import_coding_tools_mcp = ($module.Count -ge 1 -and $module[0].Trim() -eq '0.2.2')
  import_pyjwt = ($module.Count -ge 2)
  cli_help_runs = $help.Contains('Serve workspace-confined coding tools over MCP.')
  actual_http_mcp_probe_exit = $probeExit
  actual_http_mcp_tools_list = (Test-Path (Join-Path $probeDir 'tools-list.json'))
  build_time_staging_used_external_python = $true
  runtime_execution_used_embedded_python_directly = $true
  runtime_pip_install = $false
}
$result.ok = $result.import_coding_tools_mcp -and $result.import_pyjwt -and $result.cli_help_runs -and ($probeExit -eq 0) -and $result.actual_http_mcp_tools_list
$result | ConvertTo-Json -Depth 5 | Set-Content -Path $OutputPath -Encoding utf8
$result | ConvertTo-Json -Compress
if (!$result.ok) { exit 1 }
