# Regenerate the info banner on Windows. No font files are distributed.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$bitmap = New-Object Drawing.Bitmap 1200,448
$graphics = [Drawing.Graphics]::FromImage($bitmap)
$graphics.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::AntiAlias
$graphics.TextRenderingHint = [Drawing.Text.TextRenderingHint]::AntiAliasGridFit
$background = New-Object Drawing.Drawing2D.LinearGradientBrush ([Drawing.Point]::new(0,0)),([Drawing.Point]::new(1200,448)),([Drawing.Color]::FromArgb(49,49,51)),([Drawing.Color]::FromArgb(23,23,25))
$graphics.FillRectangle($background, 0, 0, 1200, 448)
$random = [Random]::new(417)
$grain = [Drawing.SolidBrush]::new([Drawing.Color]::Black)
for ($i = 0; $i -lt 130000; $i++) {
    $shade = $random.Next(60,195)
    $grain.Color = [Drawing.Color]::FromArgb($random.Next(10,45),$shade,$shade,$shade)
    $graphics.FillRectangle($grain, $random.Next(1200), $random.Next(448), 1, 1)
}
$seam = [Drawing.Pen]::new([Drawing.Color]::FromArgb(100,8,8,9),2)
$graphics.DrawLine($seam,320,0,880,448)
$graphics.DrawLine($seam,0,240,205,448)
$graphics.DrawLine($seam,0,278,165,448)
$font = [Drawing.Font]::new('Ink Free',131,[Drawing.FontStyle]::Regular,[Drawing.GraphicsUnit]::Pixel)
$format = [Drawing.StringFormat]::new()
$format.Alignment = [Drawing.StringAlignment]::Center
$format.LineAlignment = [Drawing.StringAlignment]::Center
$shadow = [Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(90,210,210,212))
$text = [Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(248,248,249))
foreach ($offset in @(-2,2)) {
    $graphics.DrawString('ATARUSTIA',$font,$shadow,[Drawing.RectangleF]::new($offset,5+$offset,1200,438),$format)
}
$graphics.DrawString('ATARUSTIA',$font,$text,[Drawing.RectangleF]::new(0,5,1200,438),$format)
$destination = Join-Path $PSScriptRoot '..\assets\atarustia.png'
[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination)) | Out-Null
$bitmap.Save($destination,[Drawing.Imaging.ImageFormat]::Png)
foreach ($item in @($text,$shadow,$format,$font,$seam,$grain,$background,$graphics,$bitmap)) { $item.Dispose() }
Write-Output 'Generated assets/atarustia.png'
