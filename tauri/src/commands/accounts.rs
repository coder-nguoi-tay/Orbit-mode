use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tauri::{AppHandle, Emitter, Manager, State};

use crate::ipc::session::ProviderRegistryState;
use crate::ipc::session::SessionState;
use crate::ipc::IpcError;
use crate::models::{AccountAuthType, AccountStatus, ProviderAccount};
use crate::services::spawn_manager::find_codex;

/// Configure a Codex child process for exactly one CLI-managed credential home.
///
/// @param command The Codex command before its subcommand is appended.
/// @param profile_home The isolated home, or None for the existing system login.
/// @return The configured child command.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
fn configure_codex_account_command(mut command: Command, profile_home: Option<&str>) -> Command {
    if let Some(home) = profile_home {
        command.env("CODEX_HOME", home);
        command.env_remove("OPENAI_API_KEY");
        command.env_remove("CODEX_ACCESS_TOKEN");
        command.env_remove("OPENAI_FEDERATION_RULE_ID");
        command.env_remove("OPENAI_IDENTITY_TOKEN_FILE");
        command.arg("-c").arg("cli_auth_credentials_store=\"file\"");
    }
    command
}

/// Ask the installed Codex CLI whether an account's selected home is authenticated.
///
/// @param account The configured profile metadata.
/// @return Available when the CLI confirms login, otherwise NeedsLogin or Unavailable.
/// @throws IpcError If the status command cannot be started.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
fn codex_login_status(account: &ProviderAccount) -> Result<AccountStatus, IpcError> {
    let executable = match find_codex() {
        Some(executable) => executable,
        None => return Ok(AccountStatus::Unavailable),
    };
    let mut command =
        configure_codex_account_command(Command::new(executable), account.profile_home.as_deref());
    let output = command.args(["login", "status"]).output()?;
    let isolated_file_present = account.profile_home.as_deref().is_none_or(|home| {
        std::fs::symlink_metadata(Path::new(home).join("auth.json"))
            .map(|metadata| metadata.file_type().is_file())
            .unwrap_or(false)
    });
    Ok(if output.status.success() && isolated_file_present {
        AccountStatus::Available
    } else {
        AccountStatus::NeedsLogin
    })
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountModelAvailability {
    pub model: crate::commands::providers::ModelInfo,
    pub efforts: Vec<String>,
}

/// Discover models in the selected account's own Codex CLI environment.
///
/// @param account The authenticated Codex profile.
/// @return Models and supported reasoning efforts reported by the CLI.
/// @throws IpcError If live account-specific discovery is unavailable.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
fn codex_account_models(
    account: &ProviderAccount,
) -> Result<Vec<AccountModelAvailability>, IpcError> {
    let executable =
        find_codex().ok_or_else(|| IpcError::Other("Codex CLI is unavailable".into()))?;
    let mut command =
        configure_codex_account_command(Command::new(executable), account.profile_home.as_deref());
    let output = command.args(["debug", "models"]).output()?;
    if !output.status.success() {
        return Err(IpcError::Other(
            "Could not verify models for the selected account".into(),
        ));
    }
    let content =
        String::from_utf8(output.stdout).map_err(|error| IpcError::Other(error.to_string()))?;
    let (models, efforts) =
        crate::commands::providers::parse_codex_models_json(&content).map_err(IpcError::Other)?;
    Ok(models
        .into_iter()
        .map(|model| AccountModelAvailability {
            efforts: efforts.get(&model.id).cloned().unwrap_or_default(),
            model,
        })
        .collect())
}

/// List models verified in one authenticated Codex profile.
///
/// @param account_id The profile to inspect.
/// @param state The session manager containing account metadata.
/// @return Live model and effort availability for that account.
/// @throws IpcError If authentication or discovery fails.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
#[tauri::command]
pub fn get_account_models(
    account_id: String,
    state: State<SessionState>,
) -> Result<Vec<AccountModelAvailability>, IpcError> {
    let account = state
        .read()
        .db
        .get_provider_account(&account_id)?
        .ok_or_else(|| IpcError::Other("Account not found".into()))?;
    if account.provider_id != "codex" || codex_login_status(&account)? != AccountStatus::Available {
        return Err(IpcError::Other("Account needs Codex authentication".into()));
    }
    codex_account_models(&account)
}

/// Start a new Codex process only after the user confirms a compatible account handoff.
///
/// @param session_id The paused session retaining its current worktree.
/// @param target_account_id The account explicitly chosen by the user.
/// @param model The model approved for the new account.
/// @param effort The approved reasoning effort, when selected.
/// @param confirmed The confirmation dialog's explicit approval.
/// @param state The shared session manager.
/// @param registry The existing provider registry.
/// @param app The app event emitter.
/// @return Success after a safe handoff has been scheduled.
/// @throws IpcError If authentication, model, effort or session validation fails.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri supplies three state/handle arguments in addition to user input.
pub fn switch_session_provider_account(
    session_id: crate::models::SessionId,
    target_account_id: String,
    model: String,
    effort: Option<String>,
    confirmed: bool,
    state: State<SessionState>,
    registry: State<ProviderRegistryState>,
    app: AppHandle,
) -> Result<(), IpcError> {
    if !confirmed {
        return Err(IpcError::Other("Account handoff was not confirmed".into()));
    }
    let account = state
        .read()
        .db
        .get_provider_account(&target_account_id)?
        .ok_or_else(|| IpcError::Other("Target account not found".into()))?;
    if account.status == AccountStatus::QuotaExceeded
        || codex_login_status(&account)? != AccountStatus::Available
    {
        return Err(IpcError::Other(
            "Target account is unauthenticated or quota-exhausted".into(),
        ));
    }
    let models = codex_account_models(&account)?;
    if model != "auto" && !model.is_empty() {
        let available_model = models
            .iter()
            .find(|available| available.model.id == model)
            .ok_or_else(|| IpcError::Other(format!("{model} is unavailable on this account")))?;
        if let Some(ref effort) = effort {
            if !available_model.efforts.contains(effort) {
                return Err(IpcError::Other(format!(
                    "{effort} effort is unavailable for {model}"
                )));
            }
        }
    }
    crate::services::session_manager::SessionManager::begin_account_handoff(
        state.0.clone(),
        app,
        session_id,
        target_account_id,
        model,
        effort,
        registry.0.clone(),
    )
    .map_err(IpcError::Other)
}

/// Show non-sensitive account transitions for a session.
///
/// @param session_id The Orbit session to inspect.
/// @param state The shared database owner.
/// @return Chronological account history.
/// @throws IpcError If SQLite cannot read the history.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
#[tauri::command]
pub fn get_session_account_history(
    session_id: crate::models::SessionId,
    state: State<SessionState>,
) -> Result<Vec<crate::models::SessionAccountEvent>, IpcError> {
    Ok(state.read().db.get_session_account_history(session_id)?)
}

/// Return the app-owned profile root without exposing it through frontend IPC.
///
/// @param app The running Orbit application.
/// @return The absolute Codex profile directory.
/// @throws IpcError If the OS app-data directory is unavailable.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
fn codex_profile_root(app: &AppHandle) -> Result<PathBuf, IpcError> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| IpcError::Other(error.to_string()))?
        .join("codex-profiles"))
}

/// Restrict a newly created profile directory to its owning OS user.
///
/// @param directory The directory to protect.
/// @return Success when the directory permissions are set.
/// @throws IpcError If the OS rejects the permission change.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
fn restrict_profile_directory(directory: &Path) -> Result<(), IpcError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// List provider profiles as non-sensitive metadata.
///
/// @param state The session manager containing the shared database.
/// @return All configured profile metadata.
/// @throws IpcError If SQLite cannot read the accounts.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
#[tauri::command]
pub fn get_provider_accounts(state: State<SessionState>) -> Result<Vec<ProviderAccount>, IpcError> {
    Ok(state.read().db.list_provider_accounts()?)
}

/// Create an empty isolated Codex home; Codex itself performs authentication later.
///
/// @param label The user's display name for the profile.
/// @param auth_type The requested Codex login method.
/// @param state The session manager containing the shared database.
/// @param app The app-data path provider.
/// @return New profile metadata with NeedsLogin status.
/// @throws IpcError If the label, path or database write is invalid.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
#[tauri::command]
pub fn create_codex_account(
    label: String,
    auth_type: AccountAuthType,
    state: State<SessionState>,
    app: AppHandle,
) -> Result<ProviderAccount, IpcError> {
    let label = label.trim();
    if label.is_empty() || label.len() > 80 {
        return Err(IpcError::Other(
            "Account label must contain 1–80 characters".into(),
        ));
    }
    if auth_type == AccountAuthType::ManagedWorkspace {
        return Err(IpcError::Other(
            "Managed workspace login is not configured".into(),
        ));
    }
    let profile_root = codex_profile_root(&app)?;
    if std::fs::symlink_metadata(&profile_root)
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(IpcError::Other("Profile root cannot be a symlink".into()));
    }
    std::fs::create_dir_all(&profile_root)?;
    if std::fs::symlink_metadata(&profile_root)?
        .file_type()
        .is_symlink()
    {
        return Err(IpcError::Other("Profile root cannot be a symlink".into()));
    }
    restrict_profile_directory(&profile_root)?;
    let account_id = uuid::Uuid::new_v4().to_string();
    let profile_home = profile_root.join(&account_id);
    std::fs::create_dir(&profile_home)?;
    restrict_profile_directory(&profile_home)?;
    let now = chrono::Utc::now().to_rfc3339();
    let account = ProviderAccount {
        id: account_id,
        provider_id: "codex".into(),
        label: label.into(),
        auth_type,
        status: AccountStatus::NeedsLogin,
        execution_scope: "local".into(),
        profile_home: Some(profile_home.to_string_lossy().into_owned()),
        is_default: false,
        created_at: now.clone(),
        updated_at: now,
        last_used_at: None,
    };
    state.read().db.create_provider_account(&account)?;
    Ok(account)
}

/// Refresh an account's authentication status using a lightweight CLI command.
///
/// @param account_id The account to check.
/// @param state The session manager containing the shared database.
/// @return Updated profile metadata.
/// @throws IpcError If the account is missing or the CLI check fails.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
#[tauri::command]
pub fn check_provider_account(
    account_id: String,
    state: State<SessionState>,
) -> Result<ProviderAccount, IpcError> {
    let database = &state.read().db;
    let mut account = database
        .get_provider_account(&account_id)?
        .ok_or_else(|| IpcError::Other("Account not found".into()))?;
    if account.provider_id != "codex" || account.execution_scope != "local" {
        return Err(IpcError::Other(
            "No health check for this provider account".into(),
        ));
    }
    let authentication_status = codex_login_status(&account)?;
    account.status = if authentication_status == AccountStatus::Available
        && matches!(
            account.status,
            AccountStatus::QuotaExceeded | AccountStatus::NearLimit
        ) {
        account.status
    } else {
        authentication_status
    };
    database.update_provider_account_status(&account_id, account.status.clone())?;
    Ok(account)
}

/// Stream temporary Codex device-login instructions, including the standalone one-time code,
/// to the account UI without persisting or logging them.
///
/// @param reader The login process output stream.
/// @param account_id The profile receiving login progress.
/// @param app The application event emitter.
/// @return Success after the stream closes.
/// @throws std::io::Error If the stream cannot be read.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
fn forward_codex_login_instructions(
    reader: impl std::io::Read,
    account_id: &str,
    app: &AppHandle,
) -> std::io::Result<()> {
    for line in std::io::BufReader::new(reader).lines() {
        let line = line?;
        if !line.trim().is_empty() && line.len() <= 300 {
            let _ = app.emit(
                "account:login-progress",
                serde_json::json!({ "accountId": account_id, "line": line }),
            );
        }
    }
    Ok(())
}

/// Authenticate one isolated Codex profile through its official CLI login flow.
///
/// @param account_id The profile to authenticate.
/// @param api_key An API key passed only through child stdin for API profiles.
/// @param state The session manager containing the shared database.
/// @param app The event emitter for temporary device-login instructions.
/// @return Updated account metadata after the CLI confirms authentication.
/// @throws IpcError If the login process fails or authentication remains unavailable.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
#[tauri::command]
pub async fn authenticate_provider_account(
    account_id: String,
    api_key: Option<String>,
    state: State<'_, SessionState>,
    app: AppHandle,
) -> Result<ProviderAccount, IpcError> {
    let database = state.read().db.clone();
    let account = database
        .get_provider_account(&account_id)?
        .ok_or_else(|| IpcError::Other("Account not found".into()))?;
    if account.provider_id != "codex" || account.profile_home.is_none() {
        return Err(IpcError::Other(
            "Only isolated Codex profiles can log in here".into(),
        ));
    }
    if (account.auth_type == AccountAuthType::ApiKey) != api_key.is_some() {
        return Err(IpcError::Other(
            "Authentication method does not match this profile".into(),
        ));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let executable =
            find_codex().ok_or_else(|| IpcError::Other("Codex CLI is unavailable".into()))?;
        let mut command = configure_codex_account_command(
            Command::new(executable),
            account.profile_home.as_deref(),
        );
        command
            .arg("login")
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if api_key.is_some() {
            command.arg("--with-api-key").stdin(Stdio::piped());
        } else {
            command.arg("--device-auth").stdin(Stdio::null());
        }
        let mut child = command.spawn()?;
        if let Some(secret) = api_key {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(secret.as_bytes())?;
                stdin.write_all(b"\n")?;
            }
        }
        if let Some(stdout) = child.stdout.take() {
            forward_codex_login_instructions(stdout, &account_id, &app)?;
        }
        let exit_status = child.wait()?;
        if !exit_status.success() {
            return Err(IpcError::Other("Codex login failed".into()));
        }
        let status = codex_login_status(&account)?;
        database.update_provider_account_status(&account_id, status.clone())?;
        if status != AccountStatus::Available {
            return Err(IpcError::Other(
                "Codex login status did not confirm authentication".into(),
            ));
        }
        let mut account = account;
        account.status = status;
        Ok(account)
    })
    .await
    .map_err(|error| IpcError::Other(error.to_string()))?
}

/// Rename a configured profile without changing its credential home.
///
/// @param account_id The account being renamed.
/// @param label The new display label.
/// @param state The session manager containing the shared database.
/// @return Updated profile metadata.
/// @throws IpcError If the label or account is invalid.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
#[tauri::command]
pub fn rename_provider_account(
    account_id: String,
    label: String,
    state: State<SessionState>,
) -> Result<ProviderAccount, IpcError> {
    let label = label.trim();
    if label.is_empty() || label.len() > 80 {
        return Err(IpcError::Other(
            "Account label must contain 1–80 characters".into(),
        ));
    }
    let database = &state.read().db;
    database.rename_provider_account(&account_id, label)?;
    database
        .get_provider_account(&account_id)?
        .ok_or_else(|| IpcError::Other("Account not found".into()))
}

/// Choose the default local account for a provider.
///
/// @param account_id The profile the user selected.
/// @param state The session manager containing the shared database.
/// @return Success after storing the provider preference.
/// @throws IpcError If the account is unavailable or missing.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
#[tauri::command]
pub fn set_default_provider_account(
    account_id: String,
    state: State<SessionState>,
) -> Result<(), IpcError> {
    let database = &state.read().db;
    let account = database
        .get_provider_account(&account_id)?
        .ok_or_else(|| IpcError::Other("Account not found".into()))?;
    if (account.profile_home.is_some() && account.status == AccountStatus::Unknown)
        || matches!(
            account.status,
            AccountStatus::NeedsLogin
                | AccountStatus::AuthExpired
                | AccountStatus::QuotaExceeded
                | AccountStatus::Unavailable
        )
    {
        return Err(IpcError::Other("Account is not available".into()));
    }
    database.set_default_provider_account(&account)?;
    Ok(())
}

/// Choose the account used by future sessions in one project.
///
/// @param project_id The project receiving the override.
/// @param account_id The compatible provider profile.
/// @param state The session manager containing the shared database.
/// @return Success after storing the project preference.
/// @throws IpcError If the account is missing or unavailable.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
#[tauri::command]
pub fn set_project_provider_account(
    project_id: i64,
    account_id: String,
    state: State<SessionState>,
) -> Result<(), IpcError> {
    let database = &state.read().db;
    let account = database
        .get_provider_account(&account_id)?
        .ok_or_else(|| IpcError::Other("Account not found".into()))?;
    if (account.profile_home.is_some() && account.status == AccountStatus::Unknown)
        || matches!(
            account.status,
            AccountStatus::NeedsLogin
                | AccountStatus::AuthExpired
                | AccountStatus::QuotaExceeded
                | AccountStatus::Unavailable
        )
    {
        return Err(IpcError::Other("Account is not available".into()));
    }
    database.set_project_provider_account(project_id, &account)?;
    Ok(())
}

/// Read a project's explicit Codex profile override.
///
/// @param project_id The project whose account preference is requested.
/// @param state The database owner.
/// @return Project override account ID, or None for provider default.
/// @throws IpcError If SQLite cannot read account settings.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
#[tauri::command]
pub fn get_project_codex_account(
    project_id: i64,
    state: State<SessionState>,
) -> Result<Option<String>, IpcError> {
    Ok(state
        .read()
        .db
        .get_project_provider_account(project_id, "codex")?)
}

/// Clear a project's Codex override so the provider default applies.
///
/// @param project_id The project whose override is cleared.
/// @param state The database owner.
/// @return Success after deleting the override.
/// @throws IpcError If SQLite cannot update account settings.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
#[tauri::command]
pub fn clear_project_codex_account(
    project_id: i64,
    state: State<SessionState>,
) -> Result<(), IpcError> {
    Ok(state
        .read()
        .db
        .clear_project_provider_account(project_id, "codex")?)
}

/// Enable or disable a configured profile in the automatic quota handoff pool.
///
/// @param account_id The account whose checkbox state is being saved.
/// @param enabled Whether Orbit may select this account after quota exhaustion.
/// @param state The session manager containing account metadata.
/// @return Success after the preference is persisted.
/// @throws IpcError If the account is missing, incompatible or unavailable.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-26
#[tauri::command]
pub fn set_provider_account_auto_handoff(
    account_id: String,
    enabled: bool,
    state: State<SessionState>,
) -> Result<(), IpcError> {
    let database = &state.read().db;
    let account = database
        .get_provider_account(&account_id)?
        .ok_or_else(|| IpcError::Other("Account not found".into()))?;
    if account.provider_id != "codex" || account.execution_scope != "local" {
        return Err(IpcError::Other(
            "Automatic quota handoff is currently available for local Codex profiles".into(),
        ));
    }
    if enabled
        && !matches!(
            account.status,
            AccountStatus::Available | AccountStatus::Busy | AccountStatus::NearLimit
        )
    {
        return Err(IpcError::Other(
            "Only an authenticated available profile can be enabled".into(),
        ));
    }
    database.set_provider_account_auto_handoff(&account_id, enabled)?;
    Ok(())
}

/// Read one profile's automatic quota handoff checkbox state.
///
/// @param account_id The account whose preference is requested.
/// @param state The session manager containing account metadata.
/// @return True when the profile is enabled in the automatic handoff pool.
/// @throws IpcError If SQLite cannot read the preference.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-26
#[tauri::command]
pub fn get_provider_account_auto_handoff(
    account_id: String,
    state: State<SessionState>,
) -> Result<bool, IpcError> {
    Ok(state
        .read()
        .db
        .provider_account_auto_handoff_enabled(&account_id)?)
}

/// Disable an account and optionally remove its CLI-managed credential directory.
///
/// @param account_id The profile to remove from future selection.
/// @param delete_credentials Whether the user separately confirmed credential deletion.
/// @param state The session manager containing the shared database.
/// @param app The trusted app-data path provider.
/// @return Number of active sessions still bound to this account.
/// @throws IpcError If deletion is unsafe or SQLite cannot update the profile.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
#[tauri::command]
pub fn remove_provider_account(
    account_id: String,
    delete_credentials: bool,
    state: State<SessionState>,
    app: AppHandle,
) -> Result<i64, IpcError> {
    let database = &state.read().db;
    let account = database
        .get_provider_account(&account_id)?
        .ok_or_else(|| IpcError::Other("Account not found".into()))?;
    if account.id == "codex-system-default" {
        return Err(IpcError::Other("System Default cannot be removed".into()));
    }
    if account.is_default {
        return Err(IpcError::Other(
            "Choose another default account before removing this profile".into(),
        ));
    }
    let active_sessions = database.count_active_account_sessions(&account_id)?;
    if delete_credentials {
        if active_sessions > 0 {
            return Err(IpcError::Other(
                "Active sessions still use this profile".into(),
            ));
        }
        let profile_home = account
            .profile_home
            .ok_or_else(|| IpcError::Other("Account has no managed profile directory".into()))?;
        let profile_root = codex_profile_root(&app)?;
        if std::fs::symlink_metadata(&profile_root)?
            .file_type()
            .is_symlink()
            || std::fs::symlink_metadata(&profile_home)?
                .file_type()
                .is_symlink()
        {
            return Err(IpcError::Other(
                "Refusing to delete a symlinked profile".into(),
            ));
        }
        let root = std::fs::canonicalize(profile_root)?;
        let home = std::fs::canonicalize(profile_home)?;
        if home.parent() != Some(root.as_path())
            || home.file_name().and_then(|name| name.to_str()) != Some(account_id.as_str())
        {
            return Err(IpcError::Other(
                "Profile path is outside Orbit's managed root".into(),
            ));
        }
        std::fs::remove_dir_all(home)?;
    }
    database.disable_provider_account(&account_id)?;
    Ok(active_sessions)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify each isolated Codex child receives only its selected credential home.
    ///
    /// @return Success when both commands use distinct child environments.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    #[test]
    fn isolated_codex_commands_use_distinct_homes() {
        let personal =
            configure_codex_account_command(Command::new("codex"), Some("/tmp/personal"));
        let work = configure_codex_account_command(Command::new("codex"), Some("/tmp/work"));
        let home = |command: &Command| {
            command
                .get_envs()
                .find(|(key, _)| *key == "CODEX_HOME")
                .and_then(|(_, value)| value)
                .map(|value| value.to_string_lossy().into_owned())
        };
        assert_eq!(home(&personal).as_deref(), Some("/tmp/personal"));
        assert_eq!(home(&work).as_deref(), Some("/tmp/work"));
        assert_ne!(home(&personal), home(&work));
        for command in [&personal, &work] {
            assert!(command
                .get_envs()
                .any(|(key, value)| key == "OPENAI_API_KEY" && value.is_none()));
            assert!(command
                .get_args()
                .any(|argument| argument == "cli_auth_credentials_store=\"file\""));
        }
    }
}
