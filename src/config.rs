use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub product_name: String,
    pub version: String,
    pub exe_name: String,
    pub identifier: Option<String>,
    pub display_name: String,
    pub description: String,
    pub icon_path: PathBuf,
    pub project_root: PathBuf,
    pub src_tauri_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
struct TauriConfig {
    pub product_name: Option<String>,
    #[serde(default)]
    pub identifier: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub bundle: Option<BundleConfig>,
}

#[derive(Debug, Deserialize)]
struct TauriConfJson {
    #[serde(rename = "productName", default)]
    pub product_name: Option<String>,
    #[serde(default)]
    pub identifier: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub app: Option<TauriAppSection>,
    #[serde(default)]
    pub bundle: Option<BundleConfig>,
    #[serde(default)]
    pub tauri: Option<TauriConfig>,
}

#[derive(Debug, Deserialize)]
struct BundleConfig {
    #[serde(default)]
    pub icon: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct TauriAppSection {
    #[serde(rename = "windows", default)]
    pub windows: Vec<TauriWindowConfig>,
}

#[derive(Debug, Deserialize)]
struct TauriWindowConfig {
    #[serde(default)]
    pub title: Option<String>,
}

/// Read tauri.conf.json or src-tauri/tauri.conf.json from the project root.
/// Falls back to package.json for name/version if tauri conf not found.
pub fn detect_config(project_path: &Path, icon_override: Option<&Path>) -> Result<AppConfig> {
    let project_root = project_path
        .canonicalize()
        .unwrap_or_else(|_| project_path.to_path_buf());

    // Try tauri v2 path first: <root>/src-tauri/tauri.conf.json, then v1: <root>/tauri.conf.json
    let (conf_data, src_tauri_dir) = {
        let v2_path = project_root.join("src-tauri").join("tauri.conf.json");
        let v1_path = project_root.join("tauri.conf.json");
        if v2_path.exists() {
            let dir = v2_path.parent().unwrap().to_path_buf();
            (std::fs::read_to_string(&v2_path).context("Failed to read tauri.conf.json in src-tauri/")?, dir)
        } else if v1_path.exists() {
            let dir = v1_path.parent().unwrap().to_path_buf(); // project_root itself
            (std::fs::read_to_string(&v1_path).context("Failed to read tauri.conf.json")?, dir)
        } else {
            anyhow::bail!("tauri.conf.json not found in project root or src-tauri/");
        }
    };

    let conf: TauriConfJson = serde_json::from_str(&conf_data)
        .context("Failed to parse tauri.conf.json")?;

    // Resolve product name from multiple possible locations
    let product_name = conf
        .product_name
        .or_else(|| conf.tauri.as_ref().and_then(|t| t.product_name.clone()))
        .or_else(|| conf.identifier.clone())
        .or_else(|| {
            // Fallback: derive from package.json name
            read_package_json_name(&project_root)
        })
        .unwrap_or_else(|| "TauriApp".to_string());

    // Resolve version
    let raw_version = conf
        .version
        .or_else(|| conf.tauri.as_ref().and_then(|t| t.version.clone()))
        .or_else(|| {
            read_package_json_version(&project_root)
        })
        .unwrap_or_else(|| "0.1.0".to_string());

    let version = normalize_version(&raw_version);

    // Cargo.toml [package] name determines the exe filename and display name
    let cargo_name = read_cargo_toml_name(&src_tauri_dir);

    let exe_name = cargo_name.clone().unwrap_or_else(|| product_name.clone());

    // Resolve display name: Cargo.toml name > tauri window title > product name
    let display_name = cargo_name
        .or_else(|| {
            conf.app
                .as_ref()
                .and_then(|app| app.windows.first())
                .and_then(|w| w.title.clone())
        })
        .unwrap_or_else(|| product_name.clone());

    let description = format!("{} application", display_name);

    // Icon path: CLI override > tauri.conf.json bundle.icon > auto-detect > default
    let icon_path = if let Some(ico) = icon_override {
        ico.to_path_buf()
    } else {
        // Read from tauri.conf.json bundle.icon (both v1 and v2 formats)
        let bundle_icons: Option<Vec<String>> = conf
            .bundle
            .as_ref()
            .and_then(|b| b.icon.clone())
            .or_else(|| {
                conf.tauri
                    .as_ref()
                    .and_then(|t| t.bundle.as_ref().and_then(|b| b.icon.clone()))
            });

        if let Some(icons) = bundle_icons {
            // Pick the first .png from the icon list, typically the largest one
            let png_icon = icons.iter().find(|i| i.ends_with(".png"));
            let icon_rel = png_icon.unwrap_or(&icons[0]);
            let resolved = src_tauri_dir.join(icon_rel);
            if resolved.exists() {
                resolved
            } else {
                // Also try relative to project root
                let pr_resolved = project_root.join(icon_rel);
                if pr_resolved.exists() {
                    pr_resolved
                } else {
                    eprintln!("  Warning: Icon from tauri.conf.json not found: {}", icon_rel);
                    fallback_icon_path(&project_root)
                }
            }
        } else {
            fallback_icon_path(&project_root)
        }
    };

    Ok(AppConfig {
        product_name: product_name.clone(),
        version,
        exe_name,
        identifier: conf.identifier.or(conf.tauri.as_ref().and_then(|t| t.identifier.clone())),
        display_name,
        description,
        icon_path,
        project_root,
        src_tauri_dir,
    })
}

fn normalize_version(version: &str) -> String {
    let parts: Vec<&str> = version.split('.').collect();
    match parts.len() {
        0 => "0.0.0.0".to_string(),
        1 => format!("{}.0.0.0", parts[0]),
        2 => format!("{}.{}.0.0", parts[0], parts[1]),
        3 => format!("{}.{}.{}.0", parts[0], parts[1], parts[2]),
        _ => version.to_string(),
    }
}

/// Read the `name` field from `src-tauri/Cargo.toml` [package] section.
fn read_cargo_toml_name(src_tauri_dir: &Path) -> Option<String> {
    let cargo_path = src_tauri_dir.join("Cargo.toml");
    let data = std::fs::read_to_string(&cargo_path).ok()?;

    let mut in_package = false;
    for line in data.lines() {
        let trimmed = line.trim();
        if trimmed == "[package]" {
            in_package = true;
            continue;
        }
        if trimmed.starts_with('[') {
            if in_package {
                break; // left [package] section
            }
            continue;
        }
        if in_package {
            if let Some(val) = trimmed.strip_prefix("name") {
                let val = val.trim_start().trim_start_matches('=').trim();
                // Remove quotes: "foo" or 'foo'
                let name = val.trim_matches('"').trim_matches('\'').to_string();
                if !name.is_empty() {
                    return Some(name);
                }
            }
        }
    }
    None
}

fn fallback_icon_path(project_root: &Path) -> PathBuf {
    let candidates = [
        project_root.join("src-tauri").join("icons").join("icon.png"),
        project_root.join("icons").join("icon.png"),
        project_root.join("app-icon.png"),
    ];
    candidates
        .iter()
        .find(|p| p.exists())
        .cloned()
        .unwrap_or_else(|| PathBuf::from("icon.png"))
}

fn read_package_json_name(project_root: &Path) -> Option<String> {
    let pkg_path = project_root.join("package.json");
    let data = std::fs::read_to_string(&pkg_path).ok()?;
    let pkg: serde_json::Value = serde_json::from_str(&data).ok()?;
    pkg.get("name")?.as_str().map(|s| s.to_string())
}

fn read_package_json_version(project_root: &Path) -> Option<String> {
    let pkg_path = project_root.join("package.json");
    let data = std::fs::read_to_string(&pkg_path).ok()?;
    let pkg: serde_json::Value = serde_json::from_str(&data).ok()?;
    pkg.get("version")?.as_str().map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_version() {
        assert_eq!(normalize_version("1"), "1.0.0.0");
        assert_eq!(normalize_version("1.2"), "1.2.0.0");
        assert_eq!(normalize_version("1.2.3"), "1.2.3.0");
        assert_eq!(normalize_version("1.2.3.4"), "1.2.3.4");
    }
}
