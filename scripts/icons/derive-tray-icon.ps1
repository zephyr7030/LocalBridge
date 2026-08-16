param(
  [string]$Source = "assets/icons/localbridge.png",
  [string]$Output = "assets/icons/localbridge-tray.ico"
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

$ExpectedSourceSha256 = "710690f2d70e3c69f13db9d4eaebc0bef5c80561c74acc7bc5a401c15c16e55a"
$CropX = 90
$CropY = 400
$CropWidth = 390
$CropHeight = 390
$FrameSizes = @(16, 20, 24, 32, 48)

$sourcePath = (Resolve-Path $Source).Path
$sourceHash = (Get-FileHash $sourcePath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($sourceHash -ne $ExpectedSourceSha256) {
  throw "LocalBridge master PNG hash drift: $sourceHash"
}

$sourceBitmap = [System.Drawing.Bitmap]::FromFile($sourcePath)
try {
  if ($sourceBitmap.Width -ne 1024 -or $sourceBitmap.Height -ne 1024) {
    throw "LocalBridge master PNG dimensions drift: $($sourceBitmap.Width)x$($sourceBitmap.Height)"
  }

  $crop = [System.Drawing.Rectangle]::new($CropX, $CropY, $CropWidth, $CropHeight)
  $frames = @()
  foreach ($size in $FrameSizes) {
    $bitmap = [System.Drawing.Bitmap]::new($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    try {
      $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
      try {
        $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
        $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
        $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
        $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $destination = [System.Drawing.Rectangle]::new(0, 0, $size, $size)
        $graphics.DrawImage($sourceBitmap, $destination, $crop, [System.Drawing.GraphicsUnit]::Pixel)
      } finally {
        $graphics.Dispose()
      }

      $memory = [System.IO.MemoryStream]::new()
      try {
        $bitmap.Save($memory, [System.Drawing.Imaging.ImageFormat]::Png)
        $frames += ,([pscustomobject]@{ Size = $size; Bytes = $memory.ToArray() })
      } finally {
        $memory.Dispose()
      }
    } finally {
      $bitmap.Dispose()
    }
  }
} finally {
  $sourceBitmap.Dispose()
}

$outputPath = [System.IO.Path]::GetFullPath((Join-Path (Get-Location) $Output))
$outputDirectory = [System.IO.Path]::GetDirectoryName($outputPath)
[System.IO.Directory]::CreateDirectory($outputDirectory) | Out-Null
$stream = [System.IO.File]::Create($outputPath)
try {
  $writer = [System.IO.BinaryWriter]::new($stream)
  try {
    $writer.Write([uint16]0)
    $writer.Write([uint16]1)
    $writer.Write([uint16]$frames.Count)
    $offset = 6 + (16 * $frames.Count)
    foreach ($frame in $frames) {
      $writer.Write([byte]$frame.Size)
      $writer.Write([byte]$frame.Size)
      $writer.Write([byte]0)
      $writer.Write([byte]0)
      $writer.Write([uint16]1)
      $writer.Write([uint16]32)
      $writer.Write([uint32]$frame.Bytes.Length)
      $writer.Write([uint32]$offset)
      $offset += $frame.Bytes.Length
    }
    foreach ($frame in $frames) {
      $writer.Write($frame.Bytes)
    }
    $writer.Flush()
  } finally {
    $writer.Dispose()
  }
} finally {
  $stream.Dispose()
}

$trayHash = (Get-FileHash $outputPath -Algorithm SHA256).Hash.ToLowerInvariant()
Write-Output "LOCALBRIDGE_TRAY_DERIVE=PASS source=$Source crop=$CropX,$CropY,$CropWidth,$CropHeight frames=$($FrameSizes -join ',') sha256=$trayHash"
