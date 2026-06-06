use std::env;

fn main() {
    if let Err(error) = run() {
        eprintln!("airs-magick: {error}");
        std::process::exit(127);
    }
}

fn run() -> Result<(), String> {
    let spec = airs_image::build_from_environment(env::args_os().skip(1))?;
    let code = airs_image::run_delegate(&spec)?;
    std::process::exit(code);
}
