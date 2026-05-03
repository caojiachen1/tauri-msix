use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::AppConfig;

/// Resolve a command name to an actual executable on Windows.
/// Tries .cmd, .bat, then .exe extensions since `std::process::Command`
/// won't find batch wrappers without the extension.
pub fn resolve_command(name: &str) -> Command {
    #[cfg(windows)]
    {
        let exts = [".cmd", ".bat", ".exe"];
        for ext in &exts {
            let full = format!("{}{}", name, ext);
            let check = Command::new("where.exe")
                .arg(&full)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if check.map(|s| s.success()).unwrap_or(false) {
                return Command::new(full);
            }
        }
    }
    Command::new(name)
}

/// Check if a command is available by trying `where.exe`.
fn is_command_available(name: &str) -> bool {
    Command::new("where.exe")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Install WinApp CLI via winget if not already present.
pub fn ensure_winapp() -> Result<()> {
    if is_command_available("winapp") {
        println!("  WinApp CLI is already installed.");
        return Ok(());
    }

    println!("  Installing Microsoft.WinAppCli via winget...");
    let status = Command::new("winget")
        .args([
            "install",
            "Microsoft.WinAppCli",
            "--accept-source-agreements",
            "--accept-package-agreements",
        ])
        .status()
        .context("Failed to install WinApp CLI via winget. Is winget available?")?;

    if !status.success() {
        anyhow::bail!("WinApp CLI installation failed.");
    }
    println!("  WinApp CLI installed successfully.");
    Ok(())
}

/// Generate a dev certificate and install it.
/// Returns the path to the generated .pfx file.
pub fn ensure_cert(project_root: &Path, publisher: &str) -> Result<PathBuf> {
    let cert_path = project_root.join("devcert.pfx");

    if cert_path.exists() {
        println!("  Certificate already exists: {}", cert_path.display());
        return Ok(cert_path);
    }

    println!("  Generating development certificate (publisher: {})...", publisher);
    let output = resolve_command("winapp")
        .args(["cert", "generate", "--publisher", publisher])
        .current_dir(project_root)
        .output()
        .context("Failed to generate certificate with 'winapp cert generate'")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("winapp cert generate failed: {}", stderr);
    }

    println!("  Installing certificate (may require admin privileges)...");
    let status = resolve_command("sudo")
        .args(["winapp", "cert", "install", "devcert.pfx"])
        .current_dir(project_root)
        .status()
        .context("Failed to install certificate. Try running with admin privileges.")?;

    if !status.success() {
        anyhow::bail!("Certificate installation failed.");
    }

    println!("  Certificate generated and installed: {}", cert_path.display());
    Ok(cert_path)
}

/// Copy user-provided certificate to project root and install it.
pub fn use_user_cert(project_root: &Path, cert_file: &Path, password: Option<&str>) -> Result<PathBuf> {
    let dest = project_root.join("devcert.pfx");

    if !cert_file.exists() {
        anyhow::bail!("Certificate file not found: {}", cert_file.display());
    }

    std::fs::copy(cert_file, &dest)
        .with_context(|| format!("Failed to copy certificate to {}", dest.display()))?;

    println!("  Installing user-provided certificate (may require admin privileges)...");

    let mut args = vec!["winapp", "cert", "install", "devcert.pfx"];
    if let Some(pw) = password {
        args.push("--password");
        args.push(pw);
    }

    let status = resolve_command("sudo")
        .current_dir(project_root)
        .args(&args)
        .status()
        .context("Failed to install certificate.")?;

    if !status.success() {
        anyhow::bail!("Certificate installation failed.");
    }

    println!("  Certificate installed.");
    Ok(dest)
}

/// Find the built Tauri exe file.
pub fn find_exe(config: &AppConfig, release: bool) -> Result<PathBuf> {
    let exe_name = format!("{}.exe", config.exe_name);

    let search_paths = if release {
        vec![
            config.src_tauri_dir.join("target").join("release").join(&exe_name),
        ]
    } else {
        vec![
            config.src_tauri_dir.join("target").join("release").join(&exe_name),
            config.src_tauri_dir.join("target").join("debug").join(&exe_name),
        ]
    };

    for path in &search_paths {
        if path.exists() {
            return Ok(path.clone());
        }
    }

    let target_dir = config.src_tauri_dir.join("target");
    if target_dir.exists() {
        if let Ok(found) = find_file_recursive(&target_dir, &exe_name) {
            return Ok(found);
        }
    }

    anyhow::bail!(
        "Executable '{}' not found. Run 'npm run tauri build' first (or use --release).",
        exe_name
    )
}

fn find_file_recursive(dir: &Path, name: &str) -> Result<PathBuf> {
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("Cannot read directory: {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if let Ok(p) = find_file_recursive(&path, name) {
                return Ok(p);
            }
        } else if path.file_name().map(|n| n == name).unwrap_or(false) {
            return Ok(path);
        }
    }
    anyhow::bail!("File {} not found under {}", name, dir.display())
}

/// Create the MSIX packing directory, copy exe, manifest, and all icon files.
/// Icons are copied from the persistent `icons/` dir in the project root
/// (generated by a previous `generate_icons` call) into msix/icons/.
pub fn create_pack_dir(config: &AppConfig, exe_path: &Path) -> Result<PathBuf> {
    let pack_dir = config.project_root.join("msix");
    let pack_icons_dir = pack_dir.join("icons");
    let persistent_icons = config.project_root.join("icons");

    // Clean and recreate
    if pack_dir.exists() {
        std::fs::remove_dir_all(&pack_dir)
            .context("Failed to clean msix directory")?;
    }
    std::fs::create_dir_all(&pack_icons_dir)
        .context("Failed to create msix/icons directory")?;

    // Copy exe
    let exe_name = format!("{}.exe", config.exe_name);
    std::fs::copy(exe_path, pack_dir.join(&exe_name))
        .with_context(|| "Failed to copy exe to pack directory")?;

    // Copy manifest (must be named AppxManifest.xml)
    let manifest_src = config.project_root.join("appxmanifest.xml");
    std::fs::copy(&manifest_src, pack_dir.join("AppxManifest.xml"))
        .with_context(|| "Failed to copy manifest to pack directory")?;

    // Copy generated icons from persistent icons/ to msix/icons/
    if persistent_icons.exists() {
        copy_dir_contents(&persistent_icons, &pack_icons_dir)
            .context("Failed to copy icon files to pack directory")?;
        println!("  Copied icons/ -> msix/icons/");
    } else {
        eprintln!("  Warning: icons/ directory not found, MSIX will use default icons.");
    }

    // Copy icon.ico from src-tauri/icons/
    let ico_candidates = [
        config.src_tauri_dir.join("icons").join("icon.ico"),
        config.project_root.join("src-tauri").join("icons").join("icon.ico"),
    ];
    for src in &ico_candidates {
        if src.exists() {
            let dest = pack_icons_dir.join("icon.ico");
            std::fs::copy(src, &dest)
                .with_context(|| format!("Failed to copy icon.ico from {}", src.display()))?;
            println!("  Copied icon.ico -> msix/icons/");
            break;
        }
    }

    println!("  Pack directory ready: {}", pack_dir.display());
    Ok(pack_dir)
}

/// Recursively copy all contents of src_dir into dst_dir.
fn copy_dir_contents(src_dir: &Path, dst_dir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(src_dir)
        .with_context(|| format!("Cannot read directory: {}", src_dir.display()))?
    {
        let entry = entry?;
        let src = entry.path();
        let dst = dst_dir.join(entry.file_name());
        if src.is_dir() {
            std::fs::create_dir_all(&dst)?;
            copy_dir_contents(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)
                .with_context(|| format!("Failed to copy {}", src.display()))?;
        }
    }
    Ok(())
}

/// Run winapp pack to create the .msix file.
pub fn pack_msix(pack_dir: &Path, cert_path: &Path, output_dir: &Path) -> Result<PathBuf> {
    println!("  Packaging MSIX...");

    let output = resolve_command("winapp")
        .args([
            "pack",
            &pack_dir.to_string_lossy(),
            "--cert",
            &cert_path.to_string_lossy(),
        ])
        .current_dir(pack_dir.parent().unwrap())
        .output()
        .context("Failed to run 'winapp pack'. Is WinApp CLI installed?")?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stdout.is_empty() {
            println!("{}", stdout);
        }
        if !stderr.is_empty() {
            eprintln!("{}", stderr);
        }
        anyhow::bail!("MSIX packaging failed.");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        println!("{}", stdout);
    }

    // Find the generated .msix file
    let parent = pack_dir.parent().unwrap();
    let msix_file = std::fs::read_dir(parent)
        .context("Failed to read output directory")?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "msix").unwrap_or(false))
        .map(|e| e.path())
        .next();

    if let Some(ref msix) = msix_file {
        let output_file = output_dir.join(msix.file_name().unwrap());
        if output_file != *msix {
            std::fs::create_dir_all(output_dir)
                .context("Failed to create output directory")?;
            std::fs::copy(msix, &output_file)
                .with_context(|| format!("Failed to copy MSIX to {}", output_dir.display()))?;
            Ok(output_file)
        } else {
            Ok(msix.clone())
        }
    } else {
        let predicted = parent.join(format!("{}.msix", pack_dir.file_name().unwrap().to_string_lossy()));
        anyhow::bail!("MSIX file not found after packaging (expected near {})", predicted.display())
    }
}

/// Clean up the temporary packing directory.
pub fn cleanup(pack_dir: &Path) {
    if pack_dir.exists() {
        match std::fs::remove_dir_all(pack_dir) {
            Ok(_) => println!("  Cleaned up: {}", pack_dir.display()),
            Err(e) => eprintln!("  Warning: Failed to clean up {}: {}", pack_dir.display(), e),
        }
    }
}
