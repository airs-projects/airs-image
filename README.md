# airs-image

`airs-magick` is a native Rust, ImageMagick-style image CLI. It does not call
ImageMagick.

Supported formats:

- read: PNG, JPEG, WebP
- write: PNG, JPEG, WebP

Supported operations:

- `-resize GEOMETRY`: `800x600`, `800x`, `x600`, `800x600!`, or `50%`
- `-crop GEOMETRY`: `WIDTHxHEIGHT+X+Y`
- `-rotate DEGREES`: `0`, `90`, `180`, or `270`
- `-strip`: re-encode pixels only, dropping metadata
- `-quality VALUE`: JPEG quality `1..=100`; accepted for PNG/WebP, but WebP is currently written lossless by the Rust encoder

Examples:

```powershell
airs-magick input.png -resize 800x600 -quality 85 output.jpg
airs-magick convert input.jpg -crop 400x300+20+10 -rotate 90 -strip output.webp
```
