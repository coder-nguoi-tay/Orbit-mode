<script lang="ts">
  import { onMount } from 'svelte';
  import type { AccountAuthType, ProviderAccount } from '../lib/types';
  import {
    authenticateProviderAccount,
    checkProviderAccount,
    clearProjectCodexAccount,
    createCodexAccount,
    getProviderAccountAutoHandoff,
    getProjectCodexAccount,
    onAccountLoginProgress,
    removeProviderAccount,
    renameProviderAccount,
    setProviderAccountAutoHandoff,
    setDefaultProviderAccount,
    setProjectProviderAccount,
  } from '../lib/tauri/accounts';
  import { listProjects } from '../lib/tauri/projects';
  import { providerAccounts, refreshProviderAccounts } from '../lib/stores/providerAccounts';
  import { providerQuotas } from '../lib/stores/usage';
  import { sessions } from '../lib/stores/sessions';
  import { usageCenterOpen } from '../lib/stores/usage';
  import { formatTimeRemaining } from '../lib/cost';

  let label = '';
  let authType: AccountAuthType = 'chat_gpt_authenticated';
  let apiKeyInputs: Record<string, string> = {};
  let accountIdInLogin: string | null = null;
  let loginInstructions: string[] = [];
  let busy = false;
  let error = '';
  let stopLoginProgress: (() => void) | null = null;
  let projects: Array<{ id: number; name: string }> = [];
  let projectAccountIds: Record<number, string> = {};
  let automaticHandoffAccounts: Record<string, boolean> = {};

  $: accounts = Object.values($providerAccounts);

  /** Load the user's account checkbox preferences without reading credentials.
   * @return Completion after all configured profile states are loaded.
   * @throws Displays backend errors in this view.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-26
   */
  async function loadAutomaticHandoffStates(): Promise<void> {
    const entries = await Promise.all(
      accounts.map(
        async (account) => [account.id, await getProviderAccountAutoHandoff(account.id)] as const
      )
    );
    automaticHandoffAccounts = Object.fromEntries(entries);
  }

  onMount(() => {
    refreshProviderAccounts()
      .then(() => loadAutomaticHandoffStates())
      .catch((failure) => (error = String(failure)));
    void listProjects()
      .then(async (availableProjects) => {
        projects = availableProjects as Array<{ id: number; name: string }>;
        const preferences = await Promise.all(
          projects.map(
            async (project) => [project.id, await getProjectCodexAccount(project.id)] as const
          )
        );
        projectAccountIds = Object.fromEntries(
          preferences.map(([projectId, accountId]) => [projectId, accountId ?? ''])
        );
      })
      .catch((failure) => (error = String(failure)));
    onAccountLoginProgress(({ accountId, line }) => {
      if (accountId === accountIdInLogin) loginInstructions = [...loginInstructions, line];
    }).then((unlisten) => (stopLoginProgress = unlisten));
    return () => stopLoginProgress?.();
  });

  /** Create a new empty CLI-managed profile.
   * @return Completion of the account list refresh.
   * @throws Displays backend errors in this view.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-25
   */
  async function addAccount(): Promise<void> {
    if (!label.trim()) return;
    busy = true;
    error = '';
    try {
      await createCodexAccount(label.trim(), authType);
      label = '';
      await refreshProviderAccounts();
      await loadAutomaticHandoffStates();
    } catch (failure) {
      error = String(failure);
    } finally {
      busy = false;
    }
  }

  /** Ask Codex CLI to authenticate one isolated account.
   * @param account The account selected by the user.
   * @return Completion after CLI login status is checked.
   * @throws Displays CLI errors in this view.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-25
   */
  async function loginAccount(account: ProviderAccount): Promise<void> {
    if (account.authType === 'api_key' && !apiKeyInputs[account.id]?.trim()) {
      error = 'Enter an API key before signing in to this API profile.';
      return;
    }
    const apiKey = account.authType === 'api_key' ? apiKeyInputs[account.id].trim() : undefined;
    apiKeyInputs = { ...apiKeyInputs, [account.id]: '' };
    accountIdInLogin = account.id;
    loginInstructions = [];
    busy = true;
    error = '';
    try {
      await authenticateProviderAccount(account.id, apiKey);
      await refreshProviderAccounts();
      await loadAutomaticHandoffStates();
    } catch (failure) {
      error = String(failure);
    } finally {
      busy = false;
      accountIdInLogin = null;
      loginInstructions = [];
    }
  }

  /** Refresh a profile's lightweight CLI authentication status.
   * @param account The account selected by the user.
   * @return Completion after the shared account map is refreshed.
   * @throws Displays CLI errors in this view.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-25
   */
  async function checkAccount(account: ProviderAccount): Promise<void> {
    busy = true;
    error = '';
    try {
      await checkProviderAccount(account.id);
      await refreshProviderAccounts();
      await loadAutomaticHandoffStates();
    } catch (failure) {
      error = String(failure);
    } finally {
      busy = false;
    }
  }

  /** Rename only the user-facing profile label.
   * @param account The account selected by the user.
   * @return Completion after the shared account map is refreshed.
   * @throws Displays backend errors in this view.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-25
   */
  async function renameAccount(account: ProviderAccount): Promise<void> {
    const nextLabel = window.prompt('Account label', account.label)?.trim();
    if (!nextLabel || nextLabel === account.label) return;
    try {
      await renameProviderAccount(account.id, nextLabel);
      await refreshProviderAccounts();
    } catch (failure) {
      error = String(failure);
    }
  }

  /** Set the profile used by future local Codex sessions.
   * @param account The account selected by the user.
   * @return Completion after the shared account map is refreshed.
   * @throws Displays backend errors in this view.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-25
   */
  async function makeDefault(account: ProviderAccount): Promise<void> {
    try {
      await setDefaultProviderAccount(account.id);
      await refreshProviderAccounts();
    } catch (failure) {
      error = String(failure);
    }
  }

  /** Save the profile selected for future Codex sessions in one project.
   * @param projectId Project receiving the account preference.
   * @param accountId Profile explicitly chosen by the user.
   * @return Completion after SQLite persists the project preference.
   * @throws Displays backend validation errors in this view.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-25
   */
  async function chooseProjectAccount(projectId: number, accountId: string): Promise<void> {
    try {
      if (accountId) await setProjectProviderAccount(projectId, accountId);
      else await clearProjectCodexAccount(projectId);
      projectAccountIds = { ...projectAccountIds, [projectId]: accountId };
    } catch (failure) {
      error = String(failure);
    }
  }

  /** Enable or disable one account in the automatic quota handoff pool.
   * @param account The account whose checkbox changed.
   * @param enabled New checkbox state.
   * @return Completion after the backend saves the preference.
   * @throws Restores the previous state and displays backend errors.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-26
   */
  async function toggleAutomaticHandoff(account: ProviderAccount, enabled: boolean): Promise<void> {
    const previous = automaticHandoffAccounts[account.id] ?? false;
    automaticHandoffAccounts = { ...automaticHandoffAccounts, [account.id]: enabled };
    try {
      await setProviderAccountAutoHandoff(account.id, enabled);
    } catch (failure) {
      automaticHandoffAccounts = { ...automaticHandoffAccounts, [account.id]: previous };
      error = String(failure);
    }
  }

  /** Disable an account while retaining its sessions and worktrees.
   * @param account The account selected by the user.
   * @return Completion after the shared account map is refreshed.
   * @throws Displays backend errors in this view.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-25
   */
  async function removeAccount(account: ProviderAccount): Promise<void> {
    const activeCount = $sessions.filter(
      (session) =>
        session.providerAccountId === account.id &&
        ['running', 'waiting', 'initializing'].includes(session.status)
    ).length;
    if (
      !window.confirm(
        `Remove ${account.label} from future selection? ${activeCount} active sessions use it. Sessions and worktrees will stay.`
      )
    )
      return;
    const deleteCredentials = window.confirm(
      'Also delete this profile’s Codex credential directory? Cancel keeps it on disk.'
    );
    try {
      await removeProviderAccount(account.id, deleteCredentials);
      await refreshProviderAccounts();
    } catch (failure) {
      error = String(failure);
    }
  }
</script>

<section class="accounts">
  <div class="accounts-heading">
    <div>
      <strong>AI accounts</strong>
      <p>Manage ChatGPT profiles and see account-specific usage.</p>
    </div>
    <button class="usage-link" on:click={() => usageCenterOpen.set(true)}>
      Open Agent Usage Control Center
    </button>
  </div>
  <p>
    Add one isolated Codex profile for each ChatGPT account. Orbit stores only the label and status;
    the official Codex login keeps the credentials in that profile's separate home.
  </p>
  <p>
    Enable <strong>Automatic quota handoff</strong> on the profiles that may be used after the current
    profile reaches an official quota limit. Orbit keeps each account's usage separate and uses each selected
    profile at most once per session.
  </p>
  <div class="add-row">
    <label>
      Profile label
      <input bind:value={label} placeholder="Personal, Work, API Project" disabled={busy} />
    </label>
    <label>
      Authentication
      <select bind:value={authType} disabled={busy}>
        <option value="chat_gpt_authenticated">ChatGPT account (Codex login)</option>
        <option value="api_key">Codex / API key</option>
      </select>
    </label>
    <button on:click={addAccount} disabled={busy || !label.trim()}>Add account</button>
  </div>
  {#if error}<p class="account-error" role="alert">{error}</p>{/if}

  {#if projects.length}
    <div class="project-preferences">
      <strong>Default Codex profile by project</strong>
      {#each projects as project}
        <label>
          {project.name}
          <select
            value={projectAccountIds[project.id] ?? ''}
            on:change={(event) => chooseProjectAccount(project.id, event.currentTarget.value)}
          >
            <option value="">Provider default</option>
            {#each accounts.filter((account) => account.providerId === 'codex' && ['available', 'busy', 'near_limit', 'unknown'].includes(account.status)) as account}
              <option value={account.id}>{account.label}</option>
            {/each}
          </select>
        </label>
      {/each}
    </div>
  {/if}

  {#each accounts as account (account.id)}
    {@const quota = $providerQuotas.find(
      (sample) =>
        sample.providerAccountId === account.id ||
        sample.accountKey === account.id ||
        (account.isDefault && (sample.accountKey === 'default' || !sample.providerAccountId))
    )}
    <article class="account-card">
      <div class="account-heading">
        <strong>{account.label}</strong>
        <span
          >{account.providerId} · {account.authType === 'api_key' ? 'API billing' : 'ChatGPT'}</span
        >
        {#if account.isDefault}<span>default</span>{/if}
      </div>
      <p>
        Status: {account.status} · Active sessions: {$sessions.filter(
          (session) =>
            session.providerAccountId === account.id &&
            ['running', 'waiting'].includes(session.status)
        ).length}
      </p>
      {#if account.id !== 'codex-system-default'}
        <label class="auto-handoff-toggle">
          <input
            type="checkbox"
            checked={automaticHandoffAccounts[account.id] ?? false}
            disabled={busy || !['available', 'busy', 'near_limit'].includes(account.status)}
            on:change={(event) => toggleAutomaticHandoff(account, event.currentTarget.checked)}
          />
          Automatic quota handoff
        </label>
      {/if}
      {#if account.authType !== 'api_key'}
        {#if quota?.fiveHour}<p>
            5h: {Math.round(quota.fiveHour.utilization * 100)}% · {quota.source}
            {#if quota.fiveHour.resetsAt}· resets {formatTimeRemaining(
                quota.fiveHour.resetsAt
              )}{/if}
          </p>{/if}
        {#if quota?.sevenDay}<p>
            7d: {Math.round(quota.sevenDay.utilization * 100)}% · {quota.source}
            {#if quota.sevenDay.resetsAt}· resets {formatTimeRemaining(
                quota.sevenDay.resetsAt
              )}{/if}
          </p>{/if}
      {:else}
        <p>API billing and rate limits are managed by the configured API project.</p>
      {/if}
      {#if accountIdInLogin === account.id && loginInstructions.length}
        <div class="login-instructions" role="status">
          {#each loginInstructions as instruction}<p>{instruction}</p>{/each}
        </div>
      {/if}
      <div class="account-actions">
        {#if account.authType === 'api_key' && account.id !== 'codex-system-default'}
          <label>
            API key
            <input
              type="password"
              value={apiKeyInputs[account.id] || ''}
              on:input={(event) =>
                (apiKeyInputs = {
                  ...apiKeyInputs,
                  [account.id]: event.currentTarget.value,
                })}
              autocomplete="off"
            />
          </label>
        {/if}
        {#if account.id !== 'codex-system-default'}
          <button on:click={() => loginAccount(account)} disabled={busy}
            >Sign in with ChatGPT</button
          >
        {/if}
        <button on:click={() => checkAccount(account)} disabled={busy}>Check</button>
        <button on:click={() => renameAccount(account)} disabled={busy}>Rename</button>
        {#if !account.isDefault && !['needs_login', 'auth_expired', 'quota_exceeded', 'unavailable'].includes(account.status)}
          <button on:click={() => makeDefault(account)} disabled={busy}>Make default</button>
        {/if}
        {#if account.id !== 'codex-system-default'}
          <button on:click={() => removeAccount(account)} disabled={busy}>Remove</button>
        {/if}
      </div>
    </article>
  {/each}
</section>

<style>
  .accounts {
    display: grid;
    gap: 14px;
    padding: 16px;
  }
  .accounts-heading {
    align-items: center;
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    justify-content: space-between;
  }
  .accounts-heading strong {
    display: block;
    color: var(--t0);
  }
  .accounts-heading p {
    margin-top: 4px;
  }
  .usage-link {
    white-space: nowrap;
  }
  .accounts p {
    margin: 0;
    color: var(--t2);
  }
  .add-row,
  .account-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: 8px;
  }
  label {
    display: grid;
    gap: 4px;
    color: var(--t2);
    font-size: var(--xs);
  }
  input,
  select {
    background: var(--bg2);
    border: 1px solid var(--bd1);
    border-radius: 5px;
    color: var(--t0);
    padding: 7px;
  }
  button {
    background: var(--bg2);
    border: 1px solid var(--bd1);
    border-radius: 5px;
    color: var(--t0);
    cursor: pointer;
    padding: 7px 10px;
  }
  button:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .account-card {
    border: 1px solid var(--bd1);
    border-radius: 8px;
    display: grid;
    gap: 9px;
    padding: 12px;
  }
  .auto-handoff-toggle {
    align-items: center;
    display: flex;
    gap: 7px;
    width: fit-content;
  }
  .auto-handoff-toggle input {
    accent-color: var(--accent);
  }
  .account-heading {
    align-items: center;
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .account-heading span {
    color: var(--t2);
    font-size: var(--xs);
  }
  .account-error {
    color: #ef6b6b !important;
  }
  .login-instructions {
    overflow-wrap: anywhere;
  }
</style>
