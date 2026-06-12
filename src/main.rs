use std::env;

fn main() {
    if let Err(error) = airs_image::magick(env::args_os().skip(1)) {
        eprintln!("airs-magick: {error}");
        std::process::exit(1);
    }
}
