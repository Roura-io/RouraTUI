use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::json;

const REPOSITORY: &str = "elGordoRoura/rouratui";
const ASSET: &str = "rouratui-darwin-arm64.tar.gz";
const CHECKSUM_ASSET: &str = "rouratui-darwin-arm64.tar.gz.sha256";

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

pub fn run(check_only: bool, json_output: bool) -> Result<(), Box<dyn std::error::Error>> {
    let release = latest_release()?;
    let current = env!("CARGO_PKG_VERSION");
    let latest = release.tag_name.trim_start_matches('v');
    let update_available = latest != current;

    if check_only || !update_available {
        if json_output {
            println!(
                "{}",
                json!({
                    "current_version": current,
                    "latest_version": latest,
                    "update_available": update_available,
                    "release_url": release.html_url,
                })
            );
        } else if update_available {
            println!("rouraTUI {latest} is available (installed: {current})");
            println!("Run `rouratui update` to install it.");
        } else {
            println!("rouraTUI is up to date ({current}).");
        }
        return Ok(());
    }

    let archive_url = asset_url(&release, ASSET)?;
    let checksum_url = asset_url(&release, CHECKSUM_ASSET)?;
    let temp = temp_dir();
    fs::create_dir_all(&temp)?;
    let archive = temp.join(ASSET);
    let checksum = temp.join(CHECKSUM_ASSET);

    download(archive_url, &archive)?;
    download(checksum_url, &checksum)?;
    verify_checksum(&temp, &checksum)?;
    extract(&archive, &temp)?;

    let downloaded_binary = temp.join("rouratui");
    if !downloaded_binary.is_file() {
        return Err(format!(
            "release archive did not contain {}",
            downloaded_binary.display()
        )
        .into());
    }
    let installed_binary = env::current_exe()?;
    install_binary(&downloaded_binary, &installed_binary)?;

    if json_output {
        println!(
            "{}",
            json!({
                "updated": true,
                "previous_version": current,
                "version": latest,
                "installed_binary": installed_binary,
            })
        );
    } else {
        println!("✓ rouraTUI updated {current} → {latest}");
        println!("  {}", installed_binary.display());
    }
    Ok(())
}

fn latest_release() -> Result<Release, Box<dyn std::error::Error>> {
    let url = format!("https://api.github.com/repos/{REPOSITORY}/releases/latest");
    let output = Command::new("curl")
        .args(["-fsSL", "-H", "Accept: application/vnd.github+json", "-H"])
        .arg("User-Agent: rouratui-updater")
        .arg(url)
        .output()?;
    if !output.status.success() {
        return Err("could not read the latest rouraTUI release from GitHub".into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn asset_url<'a>(release: &'a Release, name: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    release
        .assets
        .iter()
        .find(|asset| asset.name == name)
        .map(|asset| asset.browser_download_url.as_str())
        .ok_or_else(|| format!("release {} is missing asset {name}", release.tag_name).into())
}

fn download(url: &str, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("curl")
        .args(["-fL", "--progress-bar", "-o"])
        .arg(path)
        .arg(url)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("download failed: {url}").into())
    }
}

fn verify_checksum(dir: &Path, checksum: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("shasum")
        .args(["-a", "256", "-c"])
        .arg(checksum)
        .current_dir(dir)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err("release checksum verification failed".into())
    }
}

fn extract(archive: &Path, dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(dir)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err("could not extract the rouraTUI release".into())
    }
}

fn install_binary(source: &Path, destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let replacement = destination.with_extension("new");
    fs::copy(source, &replacement)?;
    let mut permissions = fs::metadata(&replacement)?.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
    }
    fs::set_permissions(&replacement, permissions)?;
    fs::rename(&replacement, destination)?;
    Ok(())
}

fn temp_dir() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    env::temp_dir().join(format!("rouratui-update-{}-{nonce}", std::process::id()))
}
