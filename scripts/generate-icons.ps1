# Regenerates the Faast icon PNGs from vector drawing instructions.
# Produces: icon-source.png (1024 master), app icon PNGs, Windows Store square
# logos and the small PNG sizes used by scripts/build-icon.mjs for the ICO.
# The ICO/ICNS are then assembled by:  node scripts/build-icon.mjs
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$icons = Join-Path $PSScriptRoot '..\src-tauri\icons'
$DESIGN = 1024.0

function New-RoundedPath([double]$x, [double]$y, [double]$w, [double]$h, [double]$r) {
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $d = $r * 2
  $p.AddArc([float]$x, [float]$y, [float]$d, [float]$d, 180, 90)
  $p.AddArc([float]($x + $w - $d), [float]$y, [float]$d, [float]$d, 270, 90)
  $p.AddArc([float]($x + $w - $d), [float]($y + $h - $d), [float]$d, [float]$d, 0, 90)
  $p.AddArc([float]$x, [float]($y + $h - $d), [float]$d, [float]$d, 90, 90)
  $p.CloseFigure()
  return $p
}

function New-LogoBitmap([int]$size) {
  $s = $size / $DESIGN
  $bmp = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
  $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
  $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality

  # Squircle background with brand gradient
  $m = [double](28 * $s)
  $sq = New-RoundedPath $m $m ($size - 2 * $m) ($size - 2 * $m) (232 * $s)
  $rect = New-Object System.Drawing.RectangleF($m, $m, [float]($size - 2 * $m), [float]($size - 2 * $m))
  $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    $rect,
    [System.Drawing.Color]::FromArgb(255, 59, 130, 246),
    [System.Drawing.Color]::FromArgb(255, 29, 78, 216),
    45.0)
  $g.FillPath($brush, $sq)

  # Subtle top sheen for depth. The gradient must reach full transparency
  # *within* the shape: a shorter rectangle left a hard 1px white seam at its
  # bottom edge (visible on every icon, including the Windows taskbar).
  $sheenRect = New-Object System.Drawing.RectangleF($m, $m, [float]($size - 2 * $m), [float]($size - 2 * $m))
  $sheen = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    $sheenRect,
    [System.Drawing.Color]::FromArgb(0, 255, 255, 255),
    [System.Drawing.Color]::FromArgb(0, 255, 255, 255),
    90.0)
  $blend = New-Object System.Drawing.Drawing2D.ColorBlend
  $blend.Positions = [float[]]@(0.0, 0.55, 1.0)
  $blend.Colors = [System.Drawing.Color[]]@(
    [System.Drawing.Color]::FromArgb(46, 255, 255, 255),
    [System.Drawing.Color]::FromArgb(0, 255, 255, 255),
    [System.Drawing.Color]::FromArgb(0, 255, 255, 255))
  $sheen.InterpolationColors = $blend
  $sheenPath = New-RoundedPath $m $m ($size - 2 * $m) ($size - 2 * $m) (232 * $s)
  $g.SetClip($sheenPath)
  $g.FillRectangle($sheen, $sheenRect)
  $g.ResetClip()

  # Bold download arrow (stem + head), rounded corners via a round-joined stroke
  $pts = @(
    [System.Drawing.PointF]::new([float](452 * $s), [float](244 * $s)),
    [System.Drawing.PointF]::new([float](572 * $s), [float](244 * $s)),
    [System.Drawing.PointF]::new([float](572 * $s), [float](512 * $s)),
    [System.Drawing.PointF]::new([float](726 * $s), [float](512 * $s)),
    [System.Drawing.PointF]::new([float](512 * $s), [float](798 * $s)),
    [System.Drawing.PointF]::new([float](298 * $s), [float](512 * $s)),
    [System.Drawing.PointF]::new([float](452 * $s), [float](512 * $s))
  )
  $arrow = New-Object System.Drawing.Drawing2D.GraphicsPath
  $arrow.AddPolygon($pts)
  $white = [System.Drawing.Color]::White
  $fill = New-Object System.Drawing.SolidBrush($white)
  $g.FillPath($fill, $arrow)
  $penW = [Math]::Max(1.0, 36 * $s)
  $pen = New-Object System.Drawing.Pen($white, [float]$penW)
  $pen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
  $g.DrawPath($pen, $arrow)

  $pen.Dispose(); $fill.Dispose(); $arrow.Dispose(); $brush.Dispose(); $sheen.Dispose(); $sheenPath.Dispose(); $sq.Dispose()
  $g.Dispose()
  return $bmp
}

function Save-Png([int]$size, [string]$path) {
  $bmp = New-LogoBitmap $size
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
}

# Master + app icons
Save-Png 1024 (Join-Path $icons 'icon-source.png')
Save-Png 512  (Join-Path $icons 'icon.png')
Save-Png 256  (Join-Path $icons '256x256.png')
Save-Png 256  (Join-Path $icons '128x128@2x.png')
Save-Png 128  (Join-Path $icons '128x128.png')
Save-Png 64   (Join-Path $icons '64x64.png')
Save-Png 48   (Join-Path $icons '48x48.png')
Save-Png 32   (Join-Path $icons '32x32.png')
Save-Png 24   (Join-Path $icons '24x24.png')
Save-Png 16   (Join-Path $icons '16x16.png')

# Windows Store square logos
$squares = @(
  @{ n = 'Square30x30Logo.png'; s = 30 },
  @{ n = 'Square44x44Logo.png'; s = 44 },
  @{ n = 'Square71x71Logo.png'; s = 71 },
  @{ n = 'Square89x89Logo.png'; s = 89 },
  @{ n = 'Square107x107Logo.png'; s = 107 },
  @{ n = 'Square142x142Logo.png'; s = 142 },
  @{ n = 'Square150x150Logo.png'; s = 150 },
  @{ n = 'Square284x284Logo.png'; s = 284 },
  @{ n = 'Square310x310Logo.png'; s = 310 },
  @{ n = 'StoreLogo.png'; s = 50 }
)
foreach ($sq in $squares) { Save-Png $sq.s (Join-Path $icons $sq.n) }

Write-Host 'PNG_ICONS_GENERATED'
Write-Host 'Now run: node scripts/build-icon.mjs'
