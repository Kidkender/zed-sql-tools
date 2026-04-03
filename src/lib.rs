use zed_extension_api::{
    self as zed, Architecture, Command, DownloadedFileType, GithubReleaseOptions, LanguageServerId,
    LanguageServerInstallationStatus, Os, Result, Worktree, current_platform, download_file,
    latest_github_release, make_file_executable, set_language_server_installation_status,
};

const GITHUB_REPO: &str = "kidkender/zed-sql-tools";

struct SqlToolsExtension {
    /// Cached path of the downloaded binary, to avoid re-downloading on every call.
    cached_binary_path: Option<String>,
}

impl zed::Extension for SqlToolsExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Command> {
        // Dev mode: use binary from PATH
        // Run `cargo build --release -p sql-lsp` then add target/release/ to PATH.
        if let Some(path) = worktree.which("sql-lsp") {
            return Ok(Command {
                command: path,
                args: vec![],
                env: vec![],
            });
        }

        // Production: download from GitHub Releases
        let (os, arch) = current_platform();

        let platform = match (os, arch) {
            (Os::Mac, Architecture::Aarch64) => "aarch64-apple-darwin",
            (Os::Mac, Architecture::X8664) => "x86_64-apple-darwin",
            (Os::Linux, Architecture::Aarch64) => "aarch64-unknown-linux-gnu",
            (Os::Linux, Architecture::X8664) => "x86_64-unknown-linux-gnu",
            (Os::Windows, Architecture::X8664) => "x86_64-pc-windows-msvc",
            _ => return Err(format!("unsupported platform: {os:?} {arch:?}")),
        };

        let release = latest_github_release(
            GITHUB_REPO,
            GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;

        let asset_name = format!("sql-lsp-{platform}");
        let asset = release
            .assets
            .iter()
            .find(|a| a.name == asset_name)
            .ok_or_else(|| {
                format!(
                    "release {} has no asset '{}' — available: {}",
                    release.version,
                    asset_name,
                    release
                        .assets
                        .iter()
                        .map(|a| a.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;

        // Versioned binary path so we re-download on new releases.
        let binary_path = format!("sql-lsp-{}", release.version);

        let needs_download = self
            .cached_binary_path
            .as_deref()
            .map(|p| p != binary_path)
            .unwrap_or(true)
            || !std::path::Path::new(&binary_path).exists();

        if needs_download {
            set_language_server_installation_status(
                language_server_id,
                &LanguageServerInstallationStatus::Downloading,
            );

            download_file(
                &asset.download_url,
                &binary_path,
                DownloadedFileType::Uncompressed,
            )?;

            make_file_executable(&binary_path)?;

            self.cached_binary_path = Some(binary_path.clone());
        }

        set_language_server_installation_status(
            language_server_id,
            &LanguageServerInstallationStatus::None,
        );

        Ok(Command {
            command: binary_path,
            args: vec![],
            env: vec![],
        })
    }
}

zed::register_extension!(SqlToolsExtension);
