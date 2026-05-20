# tauri-msix

A robust utility for packaging Tauri applications into the Windows MSIX format.

## Overview

`tauri-msix` is a robust CLI utility designed for packaging Tauri applications into the Windows MSIX distribution format. It bridges the gap between Tauri projects and Windows App packaging requirements without needing manually written complex manifest files.

It manages packaging configurations from your existing `tauri.conf.json` (v1 and v2), generates the indispensable `AppxManifest.xml` using `.tera` templates, and processes scaling assets like icons. It even scaffolds an `msix.bat` runner and injects `npm run msix` into your `package.json` for rapid repackaging!

## Features

- **Zero-Config Compatibility:** Automatically detects your `tauri.conf.json` and parses your `productName`, `version`, `identifier`, and more.
- **Manifest Generation:** Dynamically creates the `AppxManifest.xml` tailored specifically to your app.
- **Workflow Scripts:** Optionally writes a `msix.bat` to your layout and adds script hooks to your `package.json` and `.gitignore`.
- **Intelligent Icon Generation:** Checks to find your base `icon.png` and automates scaling / generating the required MSIX logo dimensions.
- **Certificate Binding:** Specify your own publisher or `.pfx` certificates, or fall back to an auto-created dev cert.
- **Direct Build Injection:** Automatically invokes the required `cargo build` for Tauri and uses the Windows App packaging tooling.

## Installation

As a standard Rust project, ensure you have Rust installed and simply build from source:

```bash
cargo build --release
```

Include the installed executable in your `PATH`.

> **Prerequisite:** This tool expects the Windows App packaging CLI (`winapp`) to be installed and available on your `PATH`.
> If it is missing, install it before running `tauri-msix`.

## Usage

Navigate to your target Tauri project root and launch `tauri-msix` with basic overrides, or just run it plainly:

```bash
tauri-msix [OPTIONS]
```

### Options

| Short | Long | Default | Description |
|-------|------|---------|-------------|
| `-p`  | `--path` | `.` | Path to the Tauri project root. |
| `-P`  | `--publisher` | `CN=Developer` | Publisher name for the MSIX certificate. |
| `-i`  | `--icon` | auto-detected | Path to a base 1024x1024+ PNG icon. |
| `-c`  | `--cert` | | Path to a `.pfx` signing certificate (generates a dev cert if omitted). |
| `-w`  | `--password` | | Password for the provided `.pfx` certificate. |
| `-o`  | `--output` | | Output directory for the final `.msix` file. |
| `-r`  | `--release` | `false` | Use release build target (looks for exe in `target/release`). |
|       | `--skip-build` | `false` | Skip the Rust/Tauri build step if the executable already exists. |
|       | `--keep-temp` | `false` | Keep the temporary `msix/` packaging directory artifact instead of cleaning it up. |

### Example

Generate an MSIX installer for a production build, providing your own cert identity:

```bash
tauri-msix --release --publisher "CN=MyCompany" --cert cert.pfx --password supersecret
```
