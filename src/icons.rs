use anyhow::{Context, Result};
use std::path::Path;

/// Generate MSIX icon assets from a source PNG using asset_generator crate.
/// Outputs 51 icon files into msix/icons/ directory under the project root.
pub fn generate_icons(source: &Path, output_dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    if !source.exists() {
        anyhow::bail!("Source icon not found: {}", source.display());
    }

    let source_str = source.to_str()
        .context("Source icon path contains non-UTF8 characters")?;
    let output_str = output_dir.to_str()
        .context("Output directory path contains non-UTF8 characters")?;

    println!("  Generating MSIX icons from: {}", source.display());

    let files = asset_generator::AssetGenerator::new(source_str)
        .with_context(|| format!("Failed to load source icon: {}", source.display()))?
        .with_output_dir(output_str)
        .generate_all()
        .context("Failed to generate MSIX icon assets")?;

    // Clean up duplicate files that makepri.exe would treat as conflicting resources.
    let removed = cleanup_makepri_conflicts(output_dir);
    let final_count = files.len() - removed;

    println!("  Generated {} icon files -> {} ({} conflicts removed)",
        final_count, output_dir.display(), removed);
    Ok(files)
}

/// Remove files that makepri.exe would see as conflicting resources.
///
/// MSIX makepri.exe treats files with different qualifier orderings as the same
/// resource if they resolve to the same qualifier combination. Two known conflict patterns:
///
/// 1. **scale-100**: `Foo.png` (bare) already implies scale-100, so `Foo.scale-100.png` conflicts.
/// 2. **qualifier ordering**: `Foo.targetsize-24_altform-unplated.png` and
///    `Foo.altform-unplated_targetsize-24.png` resolve to the same qualifier combination
///    (targetsize-24 + altform-unplated), so we remove the `targetsize-*_altform-*` variant
///    and keep the `altform-*_targetsize-*` one (which covers more size variants consistently).
fn cleanup_makepri_conflicts(output_dir: &Path) -> usize {
    let dir = match std::fs::read_dir(output_dir) {
        Ok(d) => d,
        Err(_) => return 0,
    };

    let mut removed = 0;
    for entry in dir.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        let should_remove =
            // pattern 1: Foo.scale-100.png (base Foo.png is already scale-100)
            name_str.ends_with(".scale-100.png") ||
            // pattern 2: Foo.targetsize-N_altform-X.png
            // (duplicates Foo.altform-X_targetsize-N.png — same qualifiers, different order)
            // Must check that .targetsize- appears BEFORE _altform- in the filename
            (|| {
                let pos_ts = name_str.find(".targetsize-");
                let pos_af = name_str.find("_altform-");
                pos_ts.is_some() && pos_af.is_some() && pos_ts < pos_af
            })();

        if should_remove {
            match std::fs::remove_file(entry.path()) {
                Ok(_) => removed += 1,
                Err(e) => eprintln!("  Warning: Failed to remove {}: {}", entry.path().display(), e),
            }
        }
    }
    removed
}
