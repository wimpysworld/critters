use std::collections::HashMap;
use std::path::{Path, PathBuf};

use zed_extension_api::{self as zed, settings::LspSettings, Result};

const SERVER_BINARY_NAME: &str = "critters-lsp";

struct CrittersExtension;

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
            return Ok(ManagedBinary {
                path: PathBuf::from(path),
                args: configured_args,
                env: merged_env(worktree, configured_env),
            });
        }

        if let Some(path) = worktree.which(SERVER_BINARY_NAME) {
            return Ok(ManagedBinary {
                path: PathBuf::from(path),
                args: configured_args,
                env: merged_env(worktree, configured_env),
            });
        }

        if let Some(path) = self.find_dev_binary() {
            return Ok(ManagedBinary {
                path,
                args: configured_args,
                env: merged_env(worktree, configured_env),
            });
        }

        self.install_binary(language_server_id)
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
    ) -> Result<ManagedBinary> {
        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::Failed(
                "Critters does not auto-download unverified GitHub release binaries. Set lsp.critters-lsp.binary.path or put critters-lsp on your PATH.".into(),
            ),
        );

        Err(
            "Critters does not auto-download unverified GitHub release binaries. Set lsp.critters-lsp.binary.path or put critters-lsp on your PATH."
                .into(),
        )
    }
}

impl zed::Extension for CrittersExtension {
    fn new() -> Self {
        Self
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

fn merged_env(
    worktree: &zed::Worktree,
    configured_env: Vec<(String, String)>,
) -> Vec<(String, String)> {
    let mut env = worktree.shell_env();
    extend_env(&mut env, configured_env);
    env
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
