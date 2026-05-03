mod config;
mod icons;
mod manifest;
mod package;
mod packager;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

/// CLI tool to add MSIX packaging capability to any Tauri project.
///
/// Reads tauri.conf.json for project metadata, generates the appxmanifest.xml,
/// creates MSIX icon assets, and packages the app into a .msix file using WinApp CLI.
///
/// Also generates a msix.bat script and injects "msix" into package.json
/// so you can repackage anytime with `npm run msix`.
#[derive(Parser, Debug)]
#[command(name = "tauri-msix", version, about, long_about = None)]
struct Cli {
    /// Path to the Tauri project root (defaults to current directory)
    #[arg(short = 'p', long, default_value = ".")]
    path: PathBuf,

    /// Publisher name for the MSIX certificate (e.g. "CN=MyCompany").
    #[arg(short = 'P', long, default_value = "CN=Developer")]
    publisher: String,

    /// Path to a 1024x1024+ PNG icon for generating MSIX assets.
    /// Auto-detected from src-tauri/icons/icon.png if not specified.
    #[arg(short = 'i', long)]
    icon: Option<PathBuf>,

    /// Path to a .pfx signing certificate.
    /// If not provided, a self-signed dev certificate is auto-generated.
    #[arg(short = 'c', long)]
    cert: Option<PathBuf>,

    /// Password for the provided .pfx certificate.
    #[arg(short = 'w', long)]
    password: Option<String>,

    /// Output directory for the final .msix file.
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,

    /// Use release build (looks for exe in target/release).
    #[arg(short = 'r', long, default_value_t = false)]
    release: bool,

    /// Skip the Tauri build step (exe must already exist).
    #[arg(long, default_value_t = false)]
    skip_build: bool,

    /// Keep the temporary msix/ packing directory after completion.
    #[arg(long, default_value_t = false)]
    keep_temp: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    println!("=== tauri-msix ===\n");

    // Step 1: Detect and validate the Tauri project
    println!("[1/6] Detecting Tauri project...");
    let app_config = config::detect_config(&cli.path, cli.icon.as_deref())?;
    println!("  Project: {} v{}", app_config.product_name, app_config.version);
    if let Some(ref id) = app_config.identifier {
        println!("  Identifier: {}", id);
    }
    println!("  Executable: {}.exe", app_config.exe_name);

    // Step 2: Generate appxmanifest.xml
    println!("\n[2/6] Generating appxmanifest.xml...");
    manifest::generate_manifest(&app_config, &cli.publisher)?;

    // Step 3: Generate msix.bat and inject npm run msix script
    println!("\n[3/6] Setting up msix.bat and npm run msix...");
    package::setup_msix_script(&app_config)?;

    // Step 4: Ensure toolchain is ready (winapp, build)
    println!("\n[4/6] Preparing toolchain...");
    packager::ensure_winapp()?;

    if cli.skip_build {
        println!("  Skipping build (--skip-build)");
    } else {
        build_tauri_project(&app_config, cli.release)?;
    }

    // Step 5: Generate icon assets (persistent, reused by msix.bat)
    println!("\n[5/6] Generating MSIX icon assets...");
    let persistent_icons = app_config.project_root.join("icons");
    std::fs::create_dir_all(&persistent_icons)?;

    let icon_files = icons::generate_icons(&app_config.icon_path, &persistent_icons)?;
    if icon_files.len() < 51 {
        eprintln!(
            "  Warning: Expected 51 icon files but generated {}. Some scale variants may be missing.",
            icon_files.len()
        );
    }

    // Step 6: Assemble pack directory and package MSIX
    println!("\n[6/6] Packaging MSIX...");

    // Certificate
    let cert_path = if let Some(ref cert_file) = cli.cert {
        packager::use_user_cert(&app_config.project_root, cert_file, cli.password.as_deref())?
    } else {
        packager::ensure_cert(&app_config.project_root, &cli.publisher)?
    };

    // Find exe
    let exe_path = packager::find_exe(&app_config, cli.release)?;
    println!("  Using exe: {}", exe_path.display());

    // Create pack directory — copies exe, manifest, icons/, and icon.ico into msix/
    let pack_dir = packager::create_pack_dir(&app_config, &exe_path)?;

    // Package
    let output_dir = cli
        .output
        .clone()
        .unwrap_or_else(|| app_config.project_root.clone());

    match packager::pack_msix(&pack_dir, &cert_path, &output_dir) {
        Ok(msix_file) => {
            // Cleanup
            if !cli.keep_temp {
                packager::cleanup(&pack_dir);
            } else {
                println!("  Kept temp directory: {}", pack_dir.display());
            }

            println!("\n=== MSIX packaging completed! ===");
            println!("  Output: {}", msix_file.display());
            println!("\n  Next time, just run: npm run msix  (or .\\scripts\\msix.bat)");
        }
        Err(e) => {
            if !cli.keep_temp {
                packager::cleanup(&pack_dir);
            }
            return Err(e);
        }
    }

    Ok(())
}

fn build_tauri_project(config: &config::AppConfig, release: bool) -> Result<()> {
    use std::process::Command;

    let pkg_path = config.project_root.join("package.json");
    let has_npm_script = if pkg_path.exists() {
        let data = std::fs::read_to_string(&pkg_path)?;
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&data) {
            pkg.get("scripts")
                .and_then(|s| s.get("tauri"))
                .is_some()
        } else {
            false
        }
    } else {
        false
    };

    if has_npm_script {
        println!("  Running: npm run tauri build...");
        let status = packager::resolve_command("npm")
            .args(["run", "tauri", "build"])
            .current_dir(&config.project_root)
            .status()
            .map_err(|e| {
                anyhow::anyhow!("Failed to run 'npm run tauri build': {}. Is npm installed?", e)
            })?;

        if !status.success() {
            anyhow::bail!("Tauri build failed. Check the output above for details.");
        }
    } else {
        println!("  No 'npm run tauri' script found. Running: cargo build...");
        let mut args = vec!["build"];
        if release {
            args.push("--release");
        }
        let status = Command::new("cargo")
            .args(&args)
            .current_dir(&config.src_tauri_dir)
            .status()
            .map_err(|e| anyhow::anyhow!("Failed to run 'cargo build': {}. Is Rust installed?", e))?;

        if !status.success() {
            anyhow::bail!("Cargo build failed. Check the output above for details.");
        }
    }

    println!("  Build completed.");
    Ok(())
}
