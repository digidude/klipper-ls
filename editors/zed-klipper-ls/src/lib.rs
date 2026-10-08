//! Starts `klipper-ls` for Klipper files.
//!
//! The server binary comes from, in order: `lsp.klipper-ls.binary.path`, a
//! `klipper-ls` on `PATH` (a local `cargo install` wins over a download), then
//! the latest klipper-ls GitHub release (see its `.github/workflows/release.yml`), downloaded
//! once per version into the extension's working directory.
//!
//! Zed settings (all optional):
//!
//! ```jsonc
//! "lsp": {
//!   "klipper-ls": {
//!     "binary": { "path": "/path/to/klipper-ls" },
//!     "initialization_options": {
//!       "klipperDocs": "~/klipper/docs",  // default: found next to your config
//!       "downloadDocs": true              // fall back to GitHub, cached
//!     }
//!   }
//! }
//! ```

use std::fs;

use zed_extension_api::{
    self as zed, serde_json, settings::LspSettings, LanguageServerId,
    LanguageServerInstallationStatus as Status, Result,
};

const SERVER: &str = "klipper-ls";
const GITHUB_REPO: &str = "digidude/klipper-ls";

struct KlipperExtension {
    /// A binary already resolved this session, so later starts skip the network.
    cached_binary_path: Option<String>,
}

/// The Rust target triple the release workflow builds for this platform.
fn release_target() -> Result<&'static str> {
    use zed::{Architecture::*, Os::*};
    match zed::current_platform() {
        (Mac, Aarch64) => Ok("aarch64-apple-darwin"),
        (Mac, X8664) => Ok("x86_64-apple-darwin"),
        (Linux, X8664) => Ok("x86_64-unknown-linux-gnu"),
        (Linux, Aarch64) => Ok("aarch64-unknown-linux-gnu"),
        _ => Err(format!(
            "no prebuilt {SERVER} for this platform; build it with `cargo install --git https://github.com/digidude/klipper-ls`"
        )),
    }
}

fn is_file(path: &str) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file())
}

/// A server downloaded by an earlier run, for when GitHub can't be reached.
fn newest_downloaded() -> Option<String> {
    let mut found: Vec<String> = fs::read_dir(".")
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let binary = format!("{name}/{SERVER}");
            (name.starts_with("klipper-ls-") && is_file(&binary)).then_some(binary)
        })
        .collect();
    found.sort();
    found.pop()
}

impl KlipperExtension {
    fn download_latest(&self, id: &LanguageServerId) -> Result<String> {
        zed::set_language_server_installation_status(id, &Status::CheckingForUpdate);
        let release = zed::latest_github_release(
            GITHUB_REPO,
            zed::GithubReleaseOptions { require_assets: true, pre_release: false },
        )?;
        let asset_name = format!("{SERVER}-{}.tar.gz", release_target()?);
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .ok_or_else(|| format!("release {} has no {asset_name}", release.version))?;

        let dir = format!("{SERVER}-{}", release.version);
        let binary = format!("{dir}/{SERVER}");
        if !is_file(&binary) {
            zed::set_language_server_installation_status(id, &Status::Downloading);
            zed::download_file(&asset.download_url, &dir, zed::DownloadedFileType::GzipTar)
                .map_err(|e| format!("downloading {asset_name}: {e}"))?;
            zed::make_file_executable(&binary)?;
            // Drop older versions.
            for entry in fs::read_dir(".").into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("klipper-ls-") && name != dir {
                    fs::remove_dir_all(entry.path()).ok();
                }
            }
        }
        Ok(binary)
    }

    fn server_binary(&mut self, id: &LanguageServerId, worktree: &zed::Worktree) -> Result<String> {
        if let Some(path) = worktree.which(SERVER) {
            return Ok(path);
        }
        if let Some(path) = self.cached_binary_path.as_ref().filter(|p| is_file(p)) {
            return Ok(path.clone());
        }
        let binary = match self.download_latest(id) {
            Ok(binary) => binary,
            Err(error) => match newest_downloaded() {
                Some(binary) => binary,
                None => {
                    let message = format!(
                        "{error}. Install the server with `cargo install --git \
                         https://github.com/digidude/klipper-ls`, or set lsp.{SERVER}.binary.path."
                    );
                    zed::set_language_server_installation_status(id, &Status::Failed(message.clone()));
                    return Err(message);
                }
            },
        };
        zed::set_language_server_installation_status(id, &Status::None);
        self.cached_binary_path = Some(binary.clone());
        Ok(binary)
    }
}

impl zed::Extension for KlipperExtension {
    fn new() -> Self {
        Self { cached_binary_path: None }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary = LspSettings::for_worktree(SERVER, worktree)
            .ok()
            .and_then(|settings| settings.binary);
        let args = binary
            .as_ref()
            .and_then(|b| b.arguments.clone())
            .unwrap_or_default();

        let command = match binary.and_then(|b| b.path) {
            Some(path) => path,
            None => self.server_binary(language_server_id, worktree)?,
        };

        Ok(zed::Command {
            command,
            args,
            env: worktree.shell_env(),
        })
    }

    fn language_server_initialization_options(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<serde_json::Value>> {
        Ok(LspSettings::for_worktree(SERVER, worktree)
            .ok()
            .and_then(|settings| settings.initialization_options))
    }
}

zed::register_extension!(KlipperExtension);
