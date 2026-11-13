# Regenerate Windows icon frames from the native sidebar's two-leaf vector mark.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$assets = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../src/chat-ui/assets'))
[xml]$vector = [IO.File]::ReadAllText((Join-Path $assets 'bonsai.svg'))
$culture = [Globalization.CultureInfo]::InvariantCulture
$sizes = @(16, 24, 32, 48, 64, 128, 256)
$frames = @()
foreach ($size in $sizes) {
    # Supersampling keeps the small shell/taskbar frames legible.
    $canvas = [Drawing.Bitmap]::new($size * 4, $size * 4)
    $bitmap = [Drawing.Bitmap]::new($size, $size)
    $graphics = [Drawing.Graphics]::FromImage($canvas)
    $scaled = [Drawing.Graphics]::FromImage($bitmap)
    $stream = [IO.MemoryStream]::new()
    try {
        $graphics.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $graphics.ScaleTransform($canvas.Width / 24.0, $canvas.Height / 24.0)
        foreach ($element in $vector.svg.path) {
            $tokens = [regex]::Matches($element.d, '[MCZ]|-?\d+(?:\.\d+)?')
            $path = [Drawing.Drawing2D.GraphicsPath]::new()
            try {
                $index = 0
                $point = [Drawing.PointF]::new(0, 0)
                while ($index -lt $tokens.Count) {
                    $command = $tokens[$index++].Value
                    switch ($command) {
                        'M' {
                            $x = [single]::Parse($tokens[$index++].Value, $culture)
                            $y = [single]::Parse($tokens[$index++].Value, $culture)
                            $point = [Drawing.PointF]::new($x, $y)
                        }
                        'C' {
                            $values = @(for ($coordinate = 0; $coordinate -lt 6; $coordinate++) {
                                [single]::Parse($tokens[$index++].Value, $culture)
                            })
                            $end = [Drawing.PointF]::new($values[4], $values[5])
                            $path.AddBezier($point, [Drawing.PointF]::new($values[0], $values[1]),
                                [Drawing.PointF]::new($values[2], $values[3]), $end)
                            $point = $end
                        }
                        'Z' { $path.CloseFigure() }
                        default { throw "Unsupported vector command: $command" }
                    }
                }
                if ($element.fill -ne 'none') {
                    $brush = [Drawing.SolidBrush]::new([Drawing.ColorTranslator]::FromHtml($element.fill))
                    try { $graphics.FillPath($brush, $path) }
                    finally { $brush.Dispose() }
                }
                if ($element.HasAttribute('stroke')) {
                    $pen = [Drawing.Pen]::new([Drawing.ColorTranslator]::FromHtml($element.stroke),
                        [single]::Parse($element.GetAttribute('stroke-width'), $culture))
                    try { $graphics.DrawPath($pen, $path) }
                    finally { $pen.Dispose() }
                }
            }
            finally { $path.Dispose() }
        }
        $scaled.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $scaled.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $scaled.DrawImage($canvas, [Drawing.Rectangle]::new(0, 0, $size, $size))
        $bitmap.Save($stream, [Drawing.Imaging.ImageFormat]::Png)
        $frames += ,$stream.ToArray()
    }
    finally {
        $stream.Dispose()
        $scaled.Dispose()
        $graphics.Dispose()
        $bitmap.Dispose()
        $canvas.Dispose()
    }
}
$output = [IO.MemoryStream]::new()
$writer = [IO.BinaryWriter]::new($output)
try {
    $writer.Write([uint16]0)
    $writer.Write([uint16]1)
    $writer.Write([uint16]$sizes.Count)
    $offset = 6 + 16 * $sizes.Count
    for ($index = 0; $index -lt $sizes.Count; $index++) {
        $dimension = [byte]($sizes[$index] % 256)
        $writer.Write($dimension)
        $writer.Write($dimension)
        $writer.Write([uint16]0)
        $writer.Write([uint16]1)
        $writer.Write([uint16]32)
        $writer.Write([uint32]$frames[$index].Length)
        $writer.Write([uint32]$offset)
        $offset += $frames[$index].Length
    }
    foreach ($frame in $frames) { $writer.Write([byte[]]$frame) }
    [IO.File]::WriteAllBytes((Join-Path $assets 'bonsai.ico'), $output.ToArray())
    [IO.File]::WriteAllBytes((Join-Path $assets 'bonsai.png'), $frames[-1])
}
finally {
    $writer.Dispose()
    $output.Dispose()
}
