<script lang="ts">
  import { onMount } from 'svelte';
  import type { Session } from '../lib/stores/sessions';
  import { providerAccounts, refreshProviderAccounts } from '../lib/stores/providerAccounts';
  import { getAccountModels, switchSessionProviderAccount } from '../lib/tauri/accounts';
  import type { AccountModelAvailability } from '../lib/tauri/accounts';
  import { gitOverview } from '../lib/tauri/git';
  import Modal from './shared/Modal.svelte';

  export let session: Session;
  export let onClose: () => void;

  let targetAccountId = '';
  let model = session.model || 'auto';
  let effort = '';
  let availableModels: AccountModelAvailability[] = [];
  let changedFileCount: number | null = null;
  let branch = session.branchName || session.gitBranch || '';
  let loadingModels = false;
  let starting = false;
  let error = '';

  $: sourceAccount = session.providerAccountId
    ? $providerAccounts[session.providerAccountId]
    : null;
  $: targetAccount = targetAccountId ? $providerAccounts[targetAccountId] : null;
  $: eligibleAccounts = Object.values($providerAccounts).filter(
    (account) =>
      account.providerId === session.provider &&
      account.executionScope === 'local' &&
      account.id !== session.providerAccountId &&
      ['available', 'busy', 'near_limit'].includes(account.status)
  );
  $: selectedModel = availableModels.find((available) => available.model.id === model);

  onMount(() => {
    refreshProviderAccounts().catch((failure) => (error = String(failure)));
    const worktree = session.worktreePath || session.cwd;
    if (worktree) {
      gitOverview(worktree)
        .then((overview) => {
          changedFileCount = overview.files.length;
          branch = overview.branch || branch;
        })
        .catch(() => {});
    }
  });

  /** Discover models and efforts within the user-selected target profile.
   * @return Completion after target-specific compatibility is shown.
   * @throws Displays discovery errors without starting a process.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-25
   */
  async function inspectTarget(): Promise<void> {
    availableModels = [];
    effort = '';
    error = '';
    if (!targetAccountId) return;
    loadingModels = true;
    try {
      availableModels = await getAccountModels(targetAccountId);
      if (model !== 'auto' && !availableModels.some((available) => available.model.id === model)) {
        model = '';
        error = 'The current model is unavailable on this account. Choose an available model.';
      }
    } catch (failure) {
      error = String(failure);
    } finally {
      loadingModels = false;
    }
  }

  /** Schedule a new process after the user confirms its account, model and worktree.
   * @return Completion after the backend accepts the explicit handoff.
   * @throws Displays backend validation errors without changing the current account.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-25
   */
  async function confirmHandoff(): Promise<void> {
    if (!targetAccount || !model) return;
    if (
      !window.confirm(
        `Switch ${session.name || 'this session'} from ${sourceAccount?.label || 'its current account'} to ${targetAccount.label}? A new Codex process will use the same worktree.`
      )
    )
      return;
    starting = true;
    error = '';
    try {
      await switchSessionProviderAccount(session.id, targetAccountId, model, effort || undefined);
    } catch (failure) {
      error = String(failure);
      starting = false;
    }
  }
</script>

<Modal title="Account handoff" width="520px" closeOnOverlayClick={false} on:close={onClose}>
  <div class="handoff">
    <p>
      Account {sourceAccount?.label || 'Unknown'} reached a usage limit. This session is paused.
    </p>
    <dl>
      <dt>From</dt>
      <dd>{session.provider} / {sourceAccount?.label || 'Unknown'}</dd>
      <dt>Worktree</dt>
      <dd>{session.worktreePath || session.cwd || 'Unknown'}</dd>
      <dt>Branch</dt>
      <dd>{branch || 'Unknown'}</dd>
      <dt>Changes</dt>
      <dd>{changedFileCount === null ? 'Checking Git' : `${changedFileCount} files`}</dd>
    </dl>
    <label>
      Target account
      <select bind:value={targetAccountId} on:change={inspectTarget} disabled={starting}>
        <option value="">Choose a configured account</option>
        {#each eligibleAccounts as account}
          <option value={account.id}>{account.label} · {account.status}</option>
        {/each}
      </select>
    </label>
    {#if targetAccountId}
      <label>
        Model
        <select bind:value={model} disabled={loadingModels || starting}>
          <option value="" disabled>Choose an available model</option>
          <option value="auto">Provider default</option>
          {#each availableModels as available}
            <option value={available.model.id}>{available.model.name}</option>
          {/each}
        </select>
      </label>
      {#if selectedModel && selectedModel.efforts.length}
        <label>
          Reasoning effort
          <select bind:value={effort} disabled={starting}>
            <option value="">Provider default</option>
            {#each selectedModel.efforts as availableEffort}
              <option value={availableEffort}>{availableEffort}</option>
            {/each}
          </select>
        </label>
      {/if}
    {/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if starting}<p role="status">Starting a new process with the selected profile…</p>{/if}
    <div class="actions">
      <button on:click={onClose} disabled={starting}>Keep paused</button>
      <button
        class="primary"
        on:click={confirmHandoff}
        disabled={starting ||
          loadingModels ||
          !targetAccountId ||
          !model ||
          (model !== 'auto' && !selectedModel)}>Switch account</button
      >
    </div>
  </div>
</Modal>

<style>
  .handoff {
    display: grid;
    gap: 14px;
    padding: 18px;
  }
  p {
    margin: 0;
    color: var(--t2);
  }
  dl {
    display: grid;
    grid-template-columns: 90px 1fr;
    gap: 7px;
    margin: 0;
    overflow-wrap: anywhere;
  }
  dt {
    color: var(--t2);
  }
  dd {
    margin: 0;
  }
  label {
    display: grid;
    gap: 6px;
  }
  select,
  button {
    background: var(--bg2);
    border: 1px solid var(--bd1);
    border-radius: 5px;
    color: var(--t0);
    padding: 8px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .primary {
    background: var(--accent);
  }
  .error {
    color: #ef6b6b;
  }
</style>
