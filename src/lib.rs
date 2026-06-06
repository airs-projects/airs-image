use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const MAGICK_ENV: &str = "AIRS_IMAGE_MAGICK";

#[derive(Debug, Eq, PartialEq)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

pub fn version() -> &'static str {
    VERSION
}

pub fn build_from_environment(
    args: impl IntoIterator<Item = OsString>,
) -> Result<CommandSpec, String> {
    let env_path = env::var_os(MAGICK_ENV);
    let path_env = env::var_os("PATH");
    let current_exe = env::current_exe().ok();
    let program_roots = default_program_roots();

    build_command(
        args,
        env_path,
        path_env,
        current_exe.as_deref(),
        &program_roots,
    )
}

pub fn build_command(
    args: impl IntoIterator<Item = OsString>,
    env_path: Option<OsString>,
    path_env: Option<OsString>,
    current_exe: Option<&Path>,
    program_roots: &[PathBuf],
) -> Result<CommandSpec, String> {
    let program = locate_magick(env_path, path_env, current_exe, program_roots)?;
    Ok(CommandSpec {
        program,
        args: args.into_iter().collect(),
    })
}

pub fn run_delegate(spec: &CommandSpec) -> Result<i32, String> {
    let status = Command::new(&spec.program)
        .args(&spec.args)
        .status()
        .map_err(|error| format!("failed to start {}: {error}", spec.program.display()))?;

    Ok(status.code().unwrap_or(1))
}

fn locate_magick(
    env_path: Option<OsString>,
    path_env: Option<OsString>,
    current_exe: Option<&Path>,
    program_roots: &[PathBuf],
) -> Result<PathBuf, String> {
    if let Some(path) = env_path.and_then(non_empty_os_string) {
        return Ok(PathBuf::from(path));
    }

    if let Some(path) = find_on_path(path_env, current_exe) {
        return Ok(path);
    }

    if let Some(path) = find_in_program_roots(program_roots, current_exe) {
        return Ok(path);
    }

    Err(format!(
        "ImageMagick 'magick' executable was not found. Install ImageMagick, add magick to PATH, or set {MAGICK_ENV}=C:\\path\\to\\magick.exe."
    ))
}

fn non_empty_os_string(value: OsString) -> Option<OsString> {
    if value.is_empty() { None } else { Some(value) }
}

fn find_on_path(path_env: Option<OsString>, current_exe: Option<&Path>) -> Option<PathBuf> {
    let path_env = path_env?;
    for dir in env::split_paths(&path_env) {
        for name in magick_binary_names() {
            let candidate = dir.join(name);
            if is_usable_delegate(&candidate, current_exe) {
                return Some(candidate);
            }
        }
    }
    None
}

fn find_in_program_roots(program_roots: &[PathBuf], current_exe: Option<&Path>) -> Option<PathBuf> {
    for root in program_roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if !name.starts_with("ImageMagick") {
                continue;
            }

            for binary in magick_binary_names() {
                let candidate = path.join(binary);
                if is_usable_delegate(&candidate, current_exe) {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn is_usable_delegate(candidate: &Path, current_exe: Option<&Path>) -> bool {
    if !candidate.is_file() {
        return false;
    }

    let Ok(candidate) = candidate.canonicalize() else {
        return true;
    };
    let Some(current_exe) = current_exe else {
        return true;
    };
    let Ok(current_exe) = current_exe.canonicalize() else {
        return true;
    };

    candidate != current_exe
}

fn magick_binary_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["magick.exe", "magick"]
    } else {
        &["magick"]
    }
}

fn default_program_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if cfg!(windows) {
        roots.push(PathBuf::from(r"C:\Program Files"));
        roots.push(PathBuf::from(r"C:\Program Files (x86)"));
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn uses_explicit_delegate_from_env() {
        let spec = build_command(
            ["convert".into(), "input.png".into(), "output.jpg".into()],
            Some(r"C:\tools\magick.exe".into()),
            None,
            None,
            &[],
        )
        .unwrap();

        assert_eq!(spec.program, PathBuf::from(r"C:\tools\magick.exe"));
        assert_eq!(
            spec.args,
            vec![
                OsString::from("convert"),
                OsString::from("input.png"),
                OsString::from("output.jpg")
            ]
        );
    }

    #[test]
    fn finds_delegate_on_path() {
        let temp = temp_dir("airs-image-path");
        fs::create_dir_all(&temp).unwrap();
        let magick = temp.join(magick_binary_names()[0]);
        File::create(&magick).unwrap();

        let spec = build_command(
            Vec::<OsString>::new(),
            None,
            Some(temp.into_os_string()),
            None,
            &[],
        )
        .unwrap();

        assert_eq!(spec.program, magick);
    }

    #[test]
    fn skips_current_exe_on_path() {
        let temp = temp_dir("airs-image-self");
        fs::create_dir_all(&temp).unwrap();
        let current = temp.join(magick_binary_names()[0]);
        File::create(&current).unwrap();

        let error = build_command(
            Vec::<OsString>::new(),
            None,
            Some(temp.into_os_string()),
            Some(&current),
            &[],
        )
        .unwrap_err();

        assert!(error.contains("ImageMagick"));
    }

    #[test]
    fn preserves_all_arguments_without_parsing() {
        let spec = build_command(
            [
                "input.png".into(),
                "-resize".into(),
                "320x200^".into(),
                "-gravity".into(),
                "center".into(),
                "output.jpg".into(),
            ],
            Some("magick".into()),
            None,
            None,
            &[],
        )
        .unwrap();

        assert_eq!(
            spec.args,
            vec![
                OsString::from("input.png"),
                OsString::from("-resize"),
                OsString::from("320x200^"),
                OsString::from("-gravity"),
                OsString::from("center"),
                OsString::from("output.jpg")
            ]
        );
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!("{prefix}-{stamp}"))
    }
}
