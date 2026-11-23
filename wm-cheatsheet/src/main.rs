//! tilekit cheatsheet — opens the OS-specific keybinding reference.
//!
//! The right cheatsheet is chosen at COMPILE time via cfg(target_os),
//! so the binary you build on each machine carries exactly one embedded
//! HTML file and zero runtime dependencies. Bound to a hotkey in
//! whkd / skhd / sxhkd (see configs/).
//!
//! Flags:
//!   --print   write the file and print its path instead of opening it

use std::env;
use std::fs;
use std::process::Command;

#[cfg(target_os = "windows")]
const HTML: &str = include_str!("../assets/windows.html");
#[cfg(target_os = "macos")]
const HTML: &str = include_str!("../assets/macos.html");
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const HTML: &str = include_str!("../assets/linux.html");

fn open(path: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    let status = Command::new("cmd").args(["/C", "start", "", path]).status()?;
    #[cfg(target_os = "macos")]
    let status = Command::new("open").arg(path).status()?;
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let status = Command::new("xdg-open").arg(path).status()?;

    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "opener exited non-zero",
        ));
    }
    Ok(())
}

fn main() {
    let path = env::temp_dir().join("tilekit-cheatsheet.html");
    if let Err(e) = fs::write(&path, HTML) {
        eprintln!("tilekit-cheatsheet: cannot write {}: {e}", path.display());
        std::process::exit(1);
    }
    let path = path.to_string_lossy().to_string();

    if env::args().any(|a| a == "--print") {
        println!("{path}");
        return;
    }
    if let Err(e) = open(&path) {
        // Headless / no opener available: still useful, print the path.
        eprintln!("tilekit-cheatsheet: could not auto-open ({e}); file at {path}");
    }
}
