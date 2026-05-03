use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tera::{Context as TeraContext, Tera};

use crate::config::AppConfig;

const MANIFEST_TEMPLATE: &str = r#"<?xml version="1.0" encoding="utf-8"?>

<Package
  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:uap2="http://schemas.microsoft.com/appx/manifest/uap/windows10/2"
  xmlns:uap3="http://schemas.microsoft.com/appx/manifest/uap/windows10/3"
  xmlns:uap10="http://schemas.microsoft.com/appx/manifest/uap/windows10/10"
  xmlns:desktop="http://schemas.microsoft.com/appx/manifest/desktop/windows10"
  xmlns:desktop6="http://schemas.microsoft.com/appx/manifest/desktop/windows10/6"
  xmlns:desktop7="http://schemas.microsoft.com/appx/manifest/desktop/windows10/7"
  xmlns:desktop10="http://schemas.microsoft.com/appx/manifest/desktop/windows10/10"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
  IgnorableNamespaces="uap uap2 uap3 uap10 desktop desktop6 desktop7 desktop10 rescap">

  <Identity
    Name="{{ app_name }}"
    Publisher="{{ publisher }}"
    Version="{{ version }}" />

  <Properties>
    <DisplayName>{{ display_name }}</DisplayName>
    <PublisherDisplayName>{{ publisher_display_name }}</PublisherDisplayName>
    <Logo>icons\StoreLogo.png</Logo>
  </Properties>

  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop"
      MinVersion="10.0.18362.0"
      MaxVersionTested="10.0.26200.0" />
  </Dependencies>

  <Resources>
    <Resource Language="en-us"/>
  </Resources>

  <Applications>
    <Application Id="{{ app_name }}"
      Executable="{{ exe_name }}.exe"
      EntryPoint="Windows.FullTrustApplication"
      uap10:TrustLevel="mediumIL"
      uap10:RuntimeBehavior="packagedClassicApp">

      <uap:VisualElements
        DisplayName="{{ display_name }}"
        Description="{{ description }}"
        BackgroundColor="transparent"
        Square150x150Logo="icons\Square150x150Logo.png"
        Square44x44Logo="icons\Square44x44Logo.png">
        <uap:DefaultTile Wide310x150Logo="icons\Wide310x150Logo.png" />
      </uap:VisualElements>

      <Extensions>
        <desktop7:Extension Category="windows.shortcut">
          <desktop7:Shortcut
            File="$(Desktop)\{{ display_name }}.lnk"
            Icon="icons\icon.ico"
            desktop10:DisplayName="{{ display_name }}"
            Description="{{ description }}" />
        </desktop7:Extension>
      </Extensions>
    </Application>
  </Applications>

  <Capabilities>
    <rescap:Capability Name="runFullTrust" />
  </Capabilities>
</Package>
"#;

/// Render appxmanifest.xml from the embedded Tera template and write it to the project root.
/// Also tries to load an external template file first (for customization), falling back to the embedded one.
pub fn generate_manifest(config: &AppConfig, publisher: &str) -> Result<()> {
    let mut tera = Tera::default();

    // Try loading an external template file first, fall back to embedded template
    let template_name = "appxmanifest.xml";
    let template_content = if let Some(path) = find_template_file() {
        std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read template at {}", path.display()))?
    } else {
        MANIFEST_TEMPLATE.to_string()
    };

    tera.add_raw_template(template_name, &template_content)
        .context("Failed to parse template")?;

    let publisher_display_name = publisher
        .strip_prefix("CN=")
        .unwrap_or(publisher);

    let mut ctx = TeraContext::new();
    ctx.insert("app_name", &config.product_name);
    ctx.insert("display_name", &config.display_name);
    ctx.insert("publisher", &publisher);
    ctx.insert("publisher_display_name", &publisher_display_name);
    ctx.insert("version", &config.version);
    ctx.insert("exe_name", &config.exe_name);
    ctx.insert("description", &config.description);

    let rendered = tera
        .render(template_name, &ctx)
        .context("Failed to render appxmanifest template")?;

    let output_path = config.project_root.join("appxmanifest.xml");
    std::fs::write(&output_path, &rendered)
        .with_context(|| format!("Failed to write manifest to {}", output_path.display()))?;

    println!("  Generated: {}", output_path.display());
    Ok(())
}

/// Find an external template file for customization.
/// Searches:
/// 1. <cwd>/templates/appxmanifest.xml.tera
/// 2. <exe_dir>/templates/appxmanifest.xml.tera
fn find_template_file() -> Option<PathBuf> {
    let candidates: &[PathBuf] = &[
        Path::new("templates").join("appxmanifest.xml.tera"),
    ];

    for candidate in candidates {
        if candidate.exists() {
            return Some(candidate.clone());
        }
    }

    // Also check relative to the executable
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let tmpl = exe_dir.join("templates").join("appxmanifest.xml.tera");
            if tmpl.exists() {
                return Some(tmpl);
            }
        }
    }

    None
}
