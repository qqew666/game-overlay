Add-Type -AssemblyName System.Drawing
$out = "D:\Projects\GameOverlay\installer\app.ico"
$dir = Split-Path $out -Parent
$sizes = @(16, 24, 32, 48, 64, 128, 256)
$pngs = @()
foreach ($s in $sizes) {
    $bmp = New-Object System.Drawing.Bitmap($s, $s)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.TextRenderingHint = 'AntiAliasGridFit'
    $g.Clear([System.Drawing.Color]::FromArgb(0, 0, 0, 0))
    $bg = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(235, 23, 26, 32))
    $r = [Math]::Max(2, [int]($s * 0.12))
    $g.FillRectangle($bg, 0, 0, $s, $s)
    $accent = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 0, 200, 255))
    $g.FillRectangle($accent, 0, 0, [Math]::Max(2, [int]($s * 0.14)), $s)
    $fc = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 240, 240, 240))
    $fs = [float]($s * 0.34)
    $font = New-Object System.Drawing.Font("Segoe UI", $fs, [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
    $fmt = New-Object System.Drawing.StringFormat
    $fmt.Alignment = 'Center'
    $fmt.LineAlignment = 'Center'
    $rect = New-Object System.Drawing.RectangleF(0, 0, [float]$s, [float]$s)
    $g.DrawString("GO", $font, $fc, $rect, $fmt)
    $g.Dispose()
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    $pngs += ,@($s, $ms.ToArray())
    $ms.Dispose()
}
$fs2 = [System.IO.File]::Create($out)
$bw = New-Object System.IO.BinaryWriter($fs2)
$bw.Write([UInt16]0)
$bw.Write([UInt16]1)
$bw.Write([UInt16]$pngs.Count)
$offset = 6 + 16 * $pngs.Count
foreach ($p in $pngs) {
    $size = $p[1].Length
    $w = $p[0]; $h = $p[0]
    if ($w -ge 256) { $w = 0 }
    if ($h -ge 256) { $h = 0 }
    $bw.Write([Byte]$w); $bw.Write([Byte]$h)
    $bw.Write([Byte]0); $bw.Write([Byte]0)
    $bw.Write([UInt16]1); $bw.Write([UInt16]32)
    $bw.Write([UInt32]$size); $bw.Write([UInt32]$offset)
    $offset += $size
}
foreach ($p in $pngs) { $bw.Write($p[1]) }
$bw.Flush(); $bw.Close(); $fs2.Close()
Write-Output "icon written: $out ($((Get-Item $out).Length) bytes)"
