$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$repo = Split-Path $PSScriptRoot -Parent
$folder = Join-Path $repo 'apps/desktop/src-tauri/installer'
$logo = [Drawing.Image]::FromFile((Join-Path $repo 'apps/desktop/src-tauri/icons/128x128@2x.png'))
try {
    foreach ($kind in @('sidebar', 'header')) {
        $width = if ($kind -eq 'sidebar') { 164 } else { 150 }
        $height = if ($kind -eq 'sidebar') { 314 } else { 57 }
        $bitmap = [Drawing.Bitmap]::new($width, $height, [Drawing.Imaging.PixelFormat]::Format24bppRgb)
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        $graphics.Clear([Drawing.Color]::FromArgb(245, 245, 247))
        $graphics.TextRenderingHint = [Drawing.Text.TextRenderingHint]::AntiAliasGridFit
        $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $font = [Drawing.Font]::new('Segoe UI', 16, [Drawing.FontStyle]::Regular, [Drawing.GraphicsUnit]::Pixel)
        $brush = [Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(29, 29, 31))
        try {
            if ($kind -eq 'sidebar') {
                $graphics.DrawImage($logo, 48, 82, 68, 68)
                $format = [Drawing.StringFormat]::new()
                $format.Alignment = [Drawing.StringAlignment]::Center
                $graphics.DrawString('Record OS', $font, $brush, [Drawing.RectangleF]::new(0, 174, 164, 32), $format)
                $format.Dispose()
            } else {
                $graphics.DrawImage($logo, 12, 10, 36, 36)
                $graphics.DrawString('Record OS', $font, $brush, 57, 18)
            }
            $bitmap.Save((Join-Path $folder "$kind.bmp"), [Drawing.Imaging.ImageFormat]::Bmp)
        } finally { $font.Dispose(); $brush.Dispose(); $graphics.Dispose(); $bitmap.Dispose() }
    }
} finally { $logo.Dispose() }
