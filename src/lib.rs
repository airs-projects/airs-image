use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageEncoder};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandPlan {
    pub input: PathBuf,
    pub output: PathBuf,
    pub operations: Vec<Operation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Operation {
    Resize(ResizeSpec),
    Crop(CropSpec),
    Rotate(i32),
    Strip,
    Quality(u8),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResizeSpec {
    Fit {
        width: Option<u32>,
        height: Option<u32>,
    },
    Exact {
        width: u32,
        height: u32,
    },
    Percent(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CropSpec {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutputFormat {
    Png,
    Jpeg,
    WebP,
}

pub fn version() -> &'static str {
    VERSION
}

pub fn magick(args: impl IntoIterator<Item = OsString>) -> Result<(), String> {
    let plan = parse_args(args)?;
    execute(&plan)
}

pub fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<CommandPlan, String> {
    let mut args: Vec<OsString> = args.into_iter().collect();
    if args.first().is_some_and(|arg| arg == "convert") {
        args.remove(0);
    }

    if args.is_empty() {
        return Err("missing input file".to_string());
    }

    if args.iter().any(|arg| arg == "-help" || arg == "--help") {
        print_usage();
        std::process::exit(0);
    }
    if args
        .iter()
        .any(|arg| arg == "-version" || arg == "--version")
    {
        println!("airs-magick {}", version());
        std::process::exit(0);
    }

    let input = PathBuf::from(&args[0]);
    let mut output = None;
    let mut operations = Vec::new();
    let mut index = 1;

    while index < args.len() {
        let arg = os_to_string(&args[index])?;
        match arg {
            "-resize" => {
                let value = require_value(&args, index, "-resize")?;
                operations.push(Operation::Resize(parse_resize(value)?));
                index += 2;
            }
            "-crop" => {
                let value = require_value(&args, index, "-crop")?;
                operations.push(Operation::Crop(parse_crop(value)?));
                index += 2;
            }
            "-rotate" => {
                let value = require_value(&args, index, "-rotate")?;
                operations.push(Operation::Rotate(parse_i32(value, "-rotate")?));
                index += 2;
            }
            "-strip" => {
                operations.push(Operation::Strip);
                index += 1;
            }
            "-quality" => {
                let value = require_value(&args, index, "-quality")?;
                operations.push(Operation::Quality(parse_quality(value)?));
                index += 2;
            }
            value if value.starts_with('-') => {
                return Err(format!("unsupported option '{value}'"));
            }
            _ => {
                if output.is_some() {
                    return Err(format!("unexpected extra argument '{arg}'"));
                }
                output = Some(PathBuf::from(&args[index]));
                index += 1;
            }
        }
    }

    let output = output.ok_or_else(|| "missing output file".to_string())?;
    Ok(CommandPlan {
        input,
        output,
        operations,
    })
}

pub fn execute(plan: &CommandPlan) -> Result<(), String> {
    let mut image = image::open(&plan.input)
        .map_err(|error| format!("{}: failed to read image: {error}", plan.input.display()))?;
    let mut quality = 90;

    for operation in &plan.operations {
        match *operation {
            Operation::Resize(ref spec) => image = resize(image, spec)?,
            Operation::Crop(spec) => image = crop(image, spec)?,
            Operation::Rotate(degrees) => image = rotate(image, degrees)?,
            Operation::Strip => {}
            Operation::Quality(value) => quality = value,
        }
    }

    write_image(&image, &plan.output, quality)
}

fn resize(image: DynamicImage, spec: &ResizeSpec) -> Result<DynamicImage, String> {
    let (width, height) = image.dimensions();
    match *spec {
        ResizeSpec::Fit {
            width: None,
            height: None,
        } => Err("resize geometry must include width, height, or percent".to_string()),
        ResizeSpec::Fit {
            width: Some(w),
            height: Some(h),
        } => Ok(image.resize(w, h, FilterType::Lanczos3)),
        ResizeSpec::Fit {
            width: Some(w),
            height: None,
        } => {
            let h = scaled_dimension(height, width, w)?;
            Ok(image.resize_exact(w, h, FilterType::Lanczos3))
        }
        ResizeSpec::Fit {
            width: None,
            height: Some(h),
        } => {
            let w = scaled_dimension(width, height, h)?;
            Ok(image.resize_exact(w, h, FilterType::Lanczos3))
        }
        ResizeSpec::Exact { width, height } => {
            Ok(image.resize_exact(width, height, FilterType::Lanczos3))
        }
        ResizeSpec::Percent(percent) => {
            if percent == 0 {
                return Err("resize percent must be greater than zero".to_string());
            }
            let w = (u64::from(width) * u64::from(percent) / 100).max(1) as u32;
            let h = (u64::from(height) * u64::from(percent) / 100).max(1) as u32;
            Ok(image.resize_exact(w, h, FilterType::Lanczos3))
        }
    }
}

fn scaled_dimension(original: u32, paired_original: u32, paired_new: u32) -> Result<u32, String> {
    if paired_original == 0 {
        return Err("cannot resize image with zero dimension".to_string());
    }
    Ok((u64::from(original) * u64::from(paired_new) / u64::from(paired_original)).max(1) as u32)
}

fn crop(image: DynamicImage, spec: CropSpec) -> Result<DynamicImage, String> {
    let (image_width, image_height) = image.dimensions();
    if spec.x >= image_width || spec.y >= image_height {
        return Err("crop offset is outside the image".to_string());
    }
    if spec.width == 0 || spec.height == 0 {
        return Err("crop width and height must be greater than zero".to_string());
    }
    if spec.x.saturating_add(spec.width) > image_width
        || spec.y.saturating_add(spec.height) > image_height
    {
        return Err("crop rectangle extends beyond the image".to_string());
    }
    Ok(image.crop_imm(spec.x, spec.y, spec.width, spec.height))
}

fn rotate(image: DynamicImage, degrees: i32) -> Result<DynamicImage, String> {
    let normalized = degrees.rem_euclid(360);
    match normalized {
        0 => Ok(image),
        90 => Ok(image.rotate90()),
        180 => Ok(image.rotate180()),
        270 => Ok(image.rotate270()),
        _ => Err(format!(
            "only right-angle rotation is supported in this native build; got {degrees}"
        )),
    }
}

fn write_image(image: &DynamicImage, output: &Path, quality: u8) -> Result<(), String> {
    let format = output_format(output)?;
    let file = File::create(output)
        .map_err(|error| format!("{}: failed to create output: {error}", output.display()))?;
    let mut writer = BufWriter::new(file);

    match format {
        OutputFormat::Png => {
            let rgba = image.to_rgba8();
            PngEncoder::new(writer)
                .write_image(
                    &rgba,
                    rgba.width(),
                    rgba.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|error| format!("{}: failed to write PNG: {error}", output.display()))
        }
        OutputFormat::Jpeg => {
            let rgb = image.to_rgb8();
            JpegEncoder::new_with_quality(writer, quality)
                .write_image(
                    &rgb,
                    rgb.width(),
                    rgb.height(),
                    image::ExtendedColorType::Rgb8,
                )
                .map_err(|error| format!("{}: failed to write JPEG: {error}", output.display()))
        }
        OutputFormat::WebP => {
            let rgba = image.to_rgba8();
            let encoded = webp::Encoder::from_rgba(&rgba, rgba.width(), rgba.height())
                .encode_simple(false, f32::from(quality))
                .map_err(|error| {
                    format!("{}: failed to encode WebP: {error:?}", output.display())
                })?;
            writer
                .write_all(&encoded)
                .map_err(|error| format!("{}: failed to write WebP: {error}", output.display()))
        }
    }
}

fn output_format(path: &Path) -> Result<OutputFormat, String> {
    match path
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => Ok(OutputFormat::Png),
        Some("jpg" | "jpeg") => Ok(OutputFormat::Jpeg),
        Some("webp") => Ok(OutputFormat::WebP),
        _ => Err(format!(
            "{}: unsupported output format; use .png, .jpg, .jpeg, or .webp",
            path.display()
        )),
    }
}

fn parse_resize(value: &str) -> Result<ResizeSpec, String> {
    if let Some(percent) = value.strip_suffix('%') {
        return Ok(ResizeSpec::Percent(parse_u32(percent, "-resize")?));
    }

    let exact = value.ends_with('!');
    let value = value.trim_end_matches('!');
    let Some((width, height)) = value.split_once('x') else {
        return Err(format!("invalid resize geometry '{value}'"));
    };

    let width = parse_optional_u32(width, "-resize")?;
    let height = parse_optional_u32(height, "-resize")?;

    if exact {
        return Ok(ResizeSpec::Exact {
            width: width.ok_or_else(|| "exact resize requires width".to_string())?,
            height: height.ok_or_else(|| "exact resize requires height".to_string())?,
        });
    }

    Ok(ResizeSpec::Fit { width, height })
}

fn parse_crop(value: &str) -> Result<CropSpec, String> {
    let Some((size, offset)) = split_crop_geometry(value) else {
        return Err(format!("invalid crop geometry '{value}'"));
    };
    let Some((width, height)) = size.split_once('x') else {
        return Err(format!("invalid crop geometry '{value}'"));
    };
    let Some((x, y)) = offset.split_once('+') else {
        return Err(format!("invalid crop offset '{offset}'"));
    };

    Ok(CropSpec {
        x: parse_u32(x, "-crop")?,
        y: parse_u32(y, "-crop")?,
        width: parse_u32(width, "-crop")?,
        height: parse_u32(height, "-crop")?,
    })
}

fn split_crop_geometry(value: &str) -> Option<(&str, &str)> {
    let plus = value.find('+')?;
    Some((&value[..plus], &value[plus + 1..]))
}

fn parse_quality(value: &str) -> Result<u8, String> {
    let quality = value
        .parse::<u8>()
        .map_err(|_| format!("invalid quality '{value}'"))?;
    if quality == 0 || quality > 100 {
        return Err("quality must be in the range 1..=100".to_string());
    }
    Ok(quality)
}

fn parse_i32(value: &str, option: &str) -> Result<i32, String> {
    value
        .parse()
        .map_err(|_| format!("{option} requires an integer value"))
}

fn parse_optional_u32(value: &str, option: &str) -> Result<Option<u32>, String> {
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(parse_u32(value, option)?))
    }
}

fn parse_u32(value: &str, option: &str) -> Result<u32, String> {
    value
        .parse()
        .map_err(|_| format!("{option} requires a positive integer"))
}

fn require_value<'a>(args: &'a [OsString], index: usize, option: &str) -> Result<&'a str, String> {
    let value = args
        .get(index + 1)
        .ok_or_else(|| format!("{option} requires a value"))?;
    if os_to_string(value)?.starts_with('-') {
        return Err(format!("{option} requires a value"));
    }
    os_to_string(value)
}

fn os_to_string(value: &OsString) -> Result<&str, String> {
    value
        .to_str()
        .ok_or_else(|| "arguments must be valid Unicode".to_string())
}

pub fn print_usage() {
    println!("Usage: airs-magick [convert] INPUT [OPTIONS] OUTPUT");
    println!("Native ImageMagick-compatible subset for PNG, JPEG, and WebP.");
    println!();
    println!("Options:");
    println!("  -resize GEOMETRY   resize: 800x600, 800x, x600, 800x600!, or 50%");
    println!("  -crop GEOMETRY     crop: WIDTHxHEIGHT+X+Y");
    println!("  -rotate DEGREES    rotate by 0, 90, 180, or 270 degrees");
    println!("  -strip             strip metadata by re-encoding pixels only");
    println!("  -quality VALUE     JPEG/WebP output quality 1..=100");
    println!("  -version           print version");
    println!("  -help              print this help");
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_magick_like_command() {
        let plan = parse_args([
            OsString::from("convert"),
            OsString::from("in.png"),
            OsString::from("-resize"),
            OsString::from("320x200"),
            OsString::from("-crop"),
            OsString::from("100x80+2+3"),
            OsString::from("-rotate"),
            OsString::from("90"),
            OsString::from("-strip"),
            OsString::from("-quality"),
            OsString::from("85"),
            OsString::from("out.webp"),
        ])
        .unwrap();

        assert_eq!(plan.input, PathBuf::from("in.png"));
        assert_eq!(plan.output, PathBuf::from("out.webp"));
        assert_eq!(
            plan.operations,
            vec![
                Operation::Resize(ResizeSpec::Fit {
                    width: Some(320),
                    height: Some(200)
                }),
                Operation::Crop(CropSpec {
                    x: 2,
                    y: 3,
                    width: 100,
                    height: 80
                }),
                Operation::Rotate(90),
                Operation::Strip,
                Operation::Quality(85)
            ]
        );
    }

    #[test]
    fn parses_resize_variants() {
        assert_eq!(parse_resize("50%").unwrap(), ResizeSpec::Percent(50));
        assert_eq!(
            parse_resize("320x200!").unwrap(),
            ResizeSpec::Exact {
                width: 320,
                height: 200
            }
        );
        assert_eq!(
            parse_resize("320x").unwrap(),
            ResizeSpec::Fit {
                width: Some(320),
                height: None
            }
        );
        assert_eq!(
            parse_resize("x200").unwrap(),
            ResizeSpec::Fit {
                width: None,
                height: Some(200)
            }
        );
    }

    #[test]
    fn rejects_unsupported_option() {
        let error = parse_args([
            OsString::from("in.png"),
            OsString::from("-blur"),
            OsString::from("2"),
            OsString::from("out.png"),
        ])
        .unwrap_err();
        assert!(error.contains("unsupported option"));
    }

    #[test]
    fn converts_png_to_jpeg_with_resize_crop_rotate() {
        let dir = temp_dir("airs-image-convert");
        fs::create_dir_all(&dir).unwrap();
        let input = dir.join("input.png");
        let output = dir.join("output.jpg");

        let image = RgbaImage::from_pixel(20, 10, Rgba([255, 0, 0, 255]));
        DynamicImage::ImageRgba8(image).save(&input).unwrap();

        let plan = parse_args([
            input.as_os_str().to_os_string(),
            OsString::from("-resize"),
            OsString::from("10x10!"),
            OsString::from("-crop"),
            OsString::from("6x4+2+3"),
            OsString::from("-rotate"),
            OsString::from("90"),
            OsString::from("-quality"),
            OsString::from("80"),
            output.as_os_str().to_os_string(),
        ])
        .unwrap();

        execute(&plan).unwrap();
        let result = image::open(output).unwrap();
        assert_eq!(result.dimensions(), (4, 6));
    }

    #[test]
    fn quality_changes_jpeg_output_size() {
        let dir = temp_dir("airs-image-quality");
        fs::create_dir_all(&dir).unwrap();
        let input = dir.join("input.png");
        let low_quality = dir.join("low.jpg");
        let high_quality = dir.join("high.jpg");

        let mut image = RgbaImage::new(64, 64);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = Rgba([(x * 3) as u8, (y * 5) as u8, ((x + y) * 2) as u8, 255]);
        }
        DynamicImage::ImageRgba8(image).save(&input).unwrap();

        execute(&CommandPlan {
            input: input.clone(),
            output: low_quality.clone(),
            operations: vec![Operation::Quality(10)],
        })
        .unwrap();
        execute(&CommandPlan {
            input,
            output: high_quality.clone(),
            operations: vec![Operation::Quality(95)],
        })
        .unwrap();

        let low_size = fs::metadata(low_quality).unwrap().len();
        let high_size = fs::metadata(high_quality).unwrap().len();
        assert!(low_size < high_size);
    }

    #[test]
    fn quality_changes_webp_output_size() {
        let dir = temp_dir("airs-image-webp-quality");
        fs::create_dir_all(&dir).unwrap();
        let input = dir.join("input.png");
        let low_quality = dir.join("low.webp");
        let high_quality = dir.join("high.webp");

        let mut image = RgbaImage::new(64, 64);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = Rgba([(x * 3) as u8, (y * 5) as u8, ((x + y) * 2) as u8, 255]);
        }
        DynamicImage::ImageRgba8(image).save(&input).unwrap();

        execute(&CommandPlan {
            input: input.clone(),
            output: low_quality.clone(),
            operations: vec![Operation::Quality(10)],
        })
        .unwrap();
        execute(&CommandPlan {
            input,
            output: high_quality.clone(),
            operations: vec![Operation::Quality(95)],
        })
        .unwrap();

        let low_size = fs::metadata(low_quality).unwrap().len();
        let high_size = fs::metadata(high_quality).unwrap().len();
        assert!(low_size < high_size);
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{stamp}"))
    }
}
