# airs-image

`airs-magick` is a Rust compatibility entry point for ImageMagick's `magick`
command. It preserves the full `magick` command surface by locating a real
ImageMagick executable and forwarding every argument unchanged.

Set `AIRS_IMAGE_MAGICK` when `magick` is not on `PATH`:

```powershell
$env:AIRS_IMAGE_MAGICK = 'C:\Program Files\ImageMagick-7.1.2-Q16-HDRI\magick.exe'
airs-magick input.png -resize 320x200 output.jpg
```

Without ImageMagick installed, `airs-magick` exits with a clear configuration
error.
