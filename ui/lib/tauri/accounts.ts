import type { AccountAuthType, AccountStatus, ProviderAccount } from '../types';
import { invoke, listen } from './invoke';

/** Read non-sensitive provider profile metadata.
 * @return Configured provider accounts.
 * @throws When the backend cannot read accounts.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function getProviderAccounts(): Promise<ProviderAccount[]> {
  return invoke('get_provider_accounts');
}

/** Create an isolated Codex profile without copying credentials.
 * @param label User-defined display name.
 * @param authType Codex login method.
 * @return Profile awaiting CLI authentication.
 * @throws When profile creation fails.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function createCodexAccount(
  label: string,
  authType: AccountAuthType
): Promise<ProviderAccount> {
  return invoke('create_codex_account', { label, authType });
}

/** Check a profile using the installed Codex CLI.
 * @param accountId Opaque profile ID.
 * @return Updated account metadata.
 * @throws When the CLI or profile cannot be checked.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function checkProviderAccount(accountId: string): Promise<ProviderAccount> {
  return invoke('check_provider_account', { accountId });
}

/** Start Codex CLI authentication for a profile.
 * @param accountId Opaque profile ID.
 * @param apiKey API key held only until the CLI receives it through stdin.
 * @return Profile after the CLI confirms authentication.
 * @throws When login fails or remains incomplete.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function authenticateProviderAccount(
  accountId: string,
  apiKey?: string
): Promise<ProviderAccount> {
  return invoke('authenticate_provider_account', { accountId, apiKey: apiKey ?? null });
}

/** Listen to temporary device-login instructions without storing them.
 * @param callback UI callback receiving the profile ID and CLI instruction.
 * @return Listener cleanup handle.
 * @throws When the event listener cannot be installed.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function onAccountLoginProgress(
  callback: (payload: { accountId: string; line: string }) => void
): Promise<() => void> {
  return listen<{ accountId: string; line: string }>('account:login-progress', (event) =>
    callback(event.payload)
  );
}

/** Rename only the account display label.
 * @param accountId Opaque profile ID.
 * @param label New user-defined label.
 * @return Updated account metadata.
 * @throws When the rename fails.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function renameProviderAccount(accountId: string, label: string): Promise<ProviderAccount> {
  return invoke('rename_provider_account', { accountId, label });
}

/** Set a provider's default profile for new local sessions.
 * @param accountId Profile selected by the user.
 * @return Completion of the saved default.
 * @throws When the account is unavailable.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function setDefaultProviderAccount(accountId: string): Promise<void> {
  return invoke('set_default_provider_account', { accountId });
}

/** Set one project's account preference.
 * @param projectId Orbit project ID.
 * @param accountId Compatible provider account.
 * @return Completion of the saved preference.
 * @throws When the account or project is invalid.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function setProjectProviderAccount(projectId: number, accountId: string): Promise<void> {
  return invoke('set_project_provider_account', { projectId, accountId });
}

/** Read a project's explicit Codex account override.
 * @param projectId Orbit project ID.
 * @return Project override ID or null when provider default applies.
 * @throws When the backend cannot read project preferences.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function getProjectCodexAccount(projectId: number): Promise<string | null> {
  return invoke('get_project_codex_account', { projectId });
}

/** Restore the provider default for new Codex sessions in a project.
 * @param projectId Orbit project ID.
 * @return Completion after the override is removed.
 * @throws When the backend cannot update project preferences.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function clearProjectCodexAccount(projectId: number): Promise<void> {
  return invoke('clear_project_codex_account', { projectId });
}

/** Save whether a Codex profile may receive an automatic quota handoff.
 * @param accountId Opaque profile ID.
 * @param enabled Checkbox state selected by the user.
 * @return Completion after the preference is persisted.
 * @throws When the profile is unavailable or the backend rejects the preference.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-26
 */
export function setProviderAccountAutoHandoff(accountId: string, enabled: boolean): Promise<void> {
  return invoke('set_provider_account_auto_handoff', { accountId, enabled });
}

/** Read whether a Codex profile is enabled for automatic quota handoff.
 * @param accountId Opaque profile ID.
 * @return True when the profile is in the automatic handoff pool.
 * @throws When the backend cannot read the preference.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-26
 */
export function getProviderAccountAutoHandoff(accountId: string): Promise<boolean> {
  return invoke('get_provider_account_auto_handoff', { accountId });
}

/** Disable a profile and optionally delete its managed CLI credentials.
 * @param accountId Opaque profile ID.
 * @param deleteCredentials Whether the user explicitly confirmed credential deletion.
 * @return Number of still-active sessions bound to the profile.
 * @throws When deletion is unsafe or the account is missing.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function removeProviderAccount(
  accountId: string,
  deleteCredentials: boolean
): Promise<number> {
  return invoke('remove_provider_account', { accountId, deleteCredentials });
}

export interface AccountModelAvailability {
  model: { id: string; name: string; context: number | null; output: number | null };
  efforts: string[];
}

export interface SessionAccountEvent {
  accountId: string;
  label: string;
  event: string;
  createdAt: string;
}

/** Read models discovered by the CLI within one account home.
 * @param accountId Profile whose model access is checked.
 * @return Live model and effort availability.
 * @throws When the account is unauthenticated or discovery fails.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function getAccountModels(accountId: string): Promise<AccountModelAvailability[]> {
  return invoke('get_account_models', { accountId });
}

/** Start a confirmed account handoff while retaining the current worktree.
 * @param sessionId Paused Orbit session ID.
 * @param targetAccountId Explicitly selected profile.
 * @param model User-approved model.
 * @param effort User-approved reasoning effort.
 * @return Completion after scheduling a new process.
 * @throws When authentication, compatibility or session validation fails.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function switchSessionProviderAccount(
  sessionId: number,
  targetAccountId: string,
  model: string,
  effort?: string
): Promise<void> {
  return invoke('switch_session_provider_account', {
    sessionId,
    targetAccountId,
    model,
    effort: effort ?? null,
    confirmed: true,
  });
}

/** Read a session's non-sensitive account transition history.
 * @param sessionId Orbit session ID.
 * @return Chronological account events.
 * @throws When the database cannot read history.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function getSessionAccountHistory(sessionId: number): Promise<SessionAccountEvent[]> {
  return invoke('get_session_account_history', { sessionId });
}

/** Observe account status transitions without notifying on every quota sample.
 * @param callback Receives the changed account and status.
 * @return Event listener cleanup handle.
 * @throws When the event listener cannot be installed.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function onProviderAccountUpdated(
  callback: (payload: { accountId: string; status: AccountStatus }) => void
): Promise<() => void> {
  return listen<{ accountId: string; status: AccountStatus }>('provider:account-updated', (event) =>
    callback(event.payload)
  );
}

/** Observe an official quota reset that makes paused sessions ready.
 * @param callback Receives the account whose sessions need manual resume.
 * @return Event listener cleanup handle.
 * @throws When the event listener cannot be installed.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function onProviderAccountReady(callback: (accountId: string) => void): Promise<() => void> {
  return listen<{ accountId: string }>('provider:account-ready', (event) =>
    callback(event.payload.accountId)
  );
}
