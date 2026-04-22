use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use zed_extension_api::{self as zed, settings::LspSettings, Result};

const SERVER_BINARY_NAME: &str = "critters-lsp";
const REPOSITORY: &str = "wimpysworld/critters";

struct CrittersExtension {
    binary_cache: Option<PathBuf>,
}

#[derive(Clone, Debug)]
struct ManagedBinary {
    path: PathBuf,
    args: Vec<String>,
    env: Vec<(String, String)>,
}

impl CrittersExtension {
    fn resolve_binary(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<ManagedBinary> {
        let settings = LspSettings::for_worktree(language_server_id.as_ref(), worktree).ok();
        let binary_settings = settings.as_ref().and_then(|value| value.binary.as_ref());
        let configured_args = binary_settings
            .and_then(|value| value.arguments.clone())
            .unwrap_or_default();
        let configured_env = binary_settings
            .and_then(|value| value.env.clone())
            .map(hash_map_to_env)
            .unwrap_or_default();

        if let Some(path) = binary_settings.and_then(|value| value.path.clone()) {
            let mut env = worktree.shell_env();
            extend_env(&mut env, configured_env);
            return Ok(ManagedBinary {
                path: PathBuf::from(path),
                args: configured_args,
                env,
            });
        }

        if let Some(path) = worktree.which(SERVER_BINARY_NAME) {
            let mut env = worktree.shell_env();
            extend_env(&mut env, configured_env);
            return Ok(ManagedBinary {
                path: PathBuf::from(path),
                args: configured_args,
                env,
            });
        }

        if let Some(path) = self.find_dev_binary() {
            return Ok(ManagedBinary {
                path,
                args: configured_args,
                env: configured_env,
            });
        }

        if let Some(path) = &self.binary_cache {
            if path.exists() {
                return Ok(ManagedBinary {
                    path: path.clone(),
                    args: configured_args,
                    env: configured_env,
                });
            }
        }

        self.install_binary(language_server_id, configured_args, configured_env)
    }

    fn find_dev_binary(&self) -> Option<PathBuf> {
        let mut candidates = vec![
            PathBuf::from("server/target/debug/critters-lsp"),
            PathBuf::from("server/target/release/critters-lsp"),
            PathBuf::from("target/debug/critters-lsp"),
            PathBuf::from("target/release/critters-lsp"),
        ];

        if cfg!(target_os = "windows") {
            candidates.extend([
                PathBuf::from("server/target/debug/critters-lsp.exe"),
                PathBuf::from("server/target/release/critters-lsp.exe"),
                PathBuf::from("target/debug/critters-lsp.exe"),
                PathBuf::from("target/release/critters-lsp.exe"),
            ]);
        }

        candidates.into_iter().find(|candidate| candidate.exists())
    }

    fn install_binary(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        args: Vec<String>,
        env: Vec<(String, String)>,
    ) -> Result<ManagedBinary> {
        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        let release = zed::latest_github_release(
            REPOSITORY,
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )
        .map_err(|error| format!("failed to fetch the latest Critters release: {error}"))?;

        let (platform, architecture) = zed::current_platform();
        let target = match (platform, architecture) {
            (zed::Os::Linux, zed::Architecture::X8664) => "x86_64-unknown-linux-gnu",
            (zed::Os::Mac, zed::Architecture::X8664) => "x86_64-apple-darwin",
            (zed::Os::Mac, zed::Architecture::Aarch64) => "aarch64-apple-darwin",
            (zed::Os::Windows, zed::Architecture::X8664) => "x86_64-pc-windows-msvc",
            _ => return Err("no managed Critters build is available for this platform yet".into()),
        };

        let is_windows = platform == zed::Os::Windows;
        let extension = if is_windows { "zip" } else { "tar.gz" };
        let asset_name = format!(
            "{SERVER_BINARY_NAME}-{}-{target}.{extension}",
            release.version
        );

        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .ok_or_else(|| format!("no Critters release asset matched {asset_name}"))?;

        let version_dir = format!("{SERVER_BINARY_NAME}-{}", release.version);
        let mut binary_path = PathBuf::from(&version_dir).join(SERVER_BINARY_NAME);
        if is_windows {
            binary_path.set_extension("exe");
        }

        if !binary_path.exists() {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );

            let download_result = (|| -> Result<()> {
                zed::download_file(
                    &asset.download_url,
                    &version_dir,
                    if is_windows {
                        zed::DownloadedFileType::Zip
                    } else {
                        zed::DownloadedFileType::GzipTar
                    },
                )
                .map_err(|error| format!("failed to download Critters binary: {error}"))?;

                zed::make_file_executable(path_to_string(&binary_path)?).map_err(|error| {
                    format!("failed to mark Critters binary executable: {error}")
                })?;

                Ok(())
            })();

            if let Err(error) = download_result {
                fs::remove_dir_all(&version_dir).ok();
                return Err(error);
            }

            cleanup_old_versions(&version_dir);
        }

        self.binary_cache = Some(binary_path.clone());

        Ok(ManagedBinary {
            path: binary_path,
            args,
            env,
        })
    }
}

impl zed::Extension for CrittersExtension {
    fn new() -> Self {
        Self { binary_cache: None }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary = self.resolve_binary(language_server_id, worktree)?;

        Ok(zed::Command {
            command: path_to_string(&binary.path)?.to_string(),
            args: binary.args,
            env: binary.env,
        })
    }

    fn language_server_initialization_options(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        LspSettings::for_worktree(language_server_id.as_ref(), worktree)
            .map(|settings| settings.initialization_options)
    }

    fn language_server_workspace_configuration(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        LspSettings::for_worktree(language_server_id.as_ref(), worktree)
            .map(|settings| settings.settings)
    }
}

fn cleanup_old_versions(active_version_dir: &str) {
    if let Ok(entries) = fs::read_dir(".") {
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }

            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };

            if name.starts_with(SERVER_BINARY_NAME) && name != active_version_dir {
                fs::remove_dir_all(entry.path()).ok();
            }
        }
    }
}

fn extend_env(target: &mut Vec<(String, String)>, source: Vec<(String, String)>) {
    for (key, value) in source {
        if let Some(existing) = target
            .iter_mut()
            .find(|(existing_key, _)| existing_key == &key)
        {
            existing.1 = value;
        } else {
            target.push((key, value));
        }
    }
}

fn hash_map_to_env(values: HashMap<String, String>) -> Vec<(String, String)> {
    values.into_iter().collect()
}

fn path_to_string(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| "binary path was not valid UTF-8".into())
}

zed::register_extension!(CrittersExtension);
