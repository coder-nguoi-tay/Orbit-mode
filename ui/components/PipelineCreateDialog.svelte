<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import { createPipeline, defaultPipelineConfig } from '../lib/tauri/pipelines';
  import { upsertPipeline, openPipeline } from '../lib/stores/pipelines';
  import { backends } from '../lib/stores/providers';

  const dispatch = createEventDispatcher<{ done: void; cancel: void }>();

  let name = '';
  let userRequest = '';
  let worktreePath = '';
  let creating = false;
  let error = '';

  // Agent config fields
  let plannerProvider = 'claude-code';
  let plannerModel = 'auto';
  let devProvider = 'claude-code';
  let devModel = 'auto';

  $: config = defaultPipelineConfig(plannerProvider, plannerModel, devProvider, devModel);

  $: providerIds = $backends.map((b) => b.id);

  function modelsFor(providerId: string): string[] {
    for (const b of $backends) {
      if (b.id === providerId) return ['auto', ...b.models.map((m) => m.id)];
      const sub = b.subProviders?.find((s) => s.id === providerId);
      if (sub) return ['auto', ...sub.models.map((m) => m.id)];
    }
    return ['auto'];
  }

  async function handleSubmit() {
    if (!name.trim() || !userRequest.trim() || !worktreePath.trim()) {
      error = 'Name, task and worktree path are required.';
      return;
    }
    creating = true;
    error = '';
    try {
      const pipeline = await createPipeline({
        name: name.trim(),
        userRequest: userRequest.trim(),
        worktreePath: worktreePath.trim(),
        config,
      });
      upsertPipeline(pipeline);
      openPipeline(pipeline.id);
      dispatch('done');
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      creating = false;
    }
  }
</script>

<div class="overlay" role="dialog" aria-modal="true" tabindex="-1">
  <div class="dialog">
    <h2 class="dialog-title">New Pipeline</h2>

    <div class="field">
      <label class="label" for="pipeline-name">Pipeline name</label>
      <input
        id="pipeline-name"
        class="input"
        type="text"
        bind:value={name}
        placeholder="e.g. PayPal integration"
        autocomplete="off"
      />
    </div>

    <div class="field">
      <label class="label" for="pipeline-request">Task / Requirement</label>
      <textarea
        id="pipeline-request"
        class="input textarea"
        bind:value={userRequest}
        rows={4}
        placeholder="Describe what you want the pipeline to implement…"
      ></textarea>
    </div>

    <div class="field">
      <label class="label" for="pipeline-worktree">Worktree / Project path</label>
      <input
        id="pipeline-worktree"
        class="input"
        type="text"
        bind:value={worktreePath}
        placeholder="/path/to/repo"
        autocomplete="off"
      />
    </div>

    <div class="agents-grid">
      <div class="agent-row">
        <span class="agent-label">Planner</span>
        <select class="select" bind:value={plannerProvider}>
          {#each providerIds as pid}
            <option value={pid}>{pid}</option>
          {/each}
        </select>
        <select class="select" bind:value={plannerModel}>
          {#each modelsFor(plannerProvider) as m}
            <option value={m}>{m}</option>
          {/each}
        </select>
      </div>
      <div class="agent-row">
        <span class="agent-label">Dev / Review / Test</span>
        <select class="select" bind:value={devProvider}>
          {#each providerIds as pid}
            <option value={pid}>{pid}</option>
          {/each}
        </select>
        <select class="select" bind:value={devModel}>
          {#each modelsFor(devProvider) as m}
            <option value={m}>{m}</option>
          {/each}
        </select>
      </div>
    </div>

    {#if error}
      <div class="error">{error}</div>
    {/if}

    <div class="actions">
      <button class="btn-ghost" on:click={() => dispatch('cancel')} disabled={creating}>
        Cancel
      </button>
      <button class="btn-primary" on:click={handleSubmit} disabled={creating}>
        {creating ? 'Creating…' : 'Create Pipeline'}
      </button>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .dialog {
    background: var(--bg);
    border: 1px solid var(--bd1);
    border-radius: 10px;
    padding: 24px 28px;
    width: 480px;
    max-width: 96vw;
    display: flex;
    flex-direction: column;
    gap: 16px;
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.4);
  }

  .dialog-title {
    font-size: 15px;
    font-weight: 600;
    color: var(--t0);
    margin: 0;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .label {
    font-size: 11px;
    color: var(--t2);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .input {
    background: var(--bg2);
    border: 1px solid var(--bd);
    border-radius: 6px;
    color: var(--t0);
    font-size: 13px;
    font-family: inherit;
    padding: 8px 10px;
    outline: none;
    transition: border-color 0.15s;
    resize: none;
  }
  .input:focus {
    border-color: var(--ac);
  }
  .textarea {
    font-family: inherit;
    line-height: 1.5;
  }

  .agents-grid {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .agent-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .agent-label {
    font-size: 11px;
    color: var(--t2);
    width: 130px;
    flex-shrink: 0;
  }
  .select {
    flex: 1;
    background: var(--bg2);
    border: 1px solid var(--bd);
    border-radius: 5px;
    color: var(--t0);
    font-size: 12px;
    font-family: var(--mono);
    padding: 5px 8px;
    outline: none;
    cursor: pointer;
  }
  .select:focus {
    border-color: var(--ac);
  }

  .error {
    font-size: 12px;
    color: var(--error, #f87171);
    background: color-mix(in srgb, var(--error, #f87171), transparent 88%);
    border: 1px solid color-mix(in srgb, var(--error, #f87171), transparent 70%);
    border-radius: 5px;
    padding: 6px 10px;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 4px;
  }

  .btn-ghost {
    background: transparent;
    border: 1px solid var(--bd);
    border-radius: 6px;
    padding: 7px 14px;
    font-size: 12px;
    color: var(--t2);
    cursor: pointer;
    transition: all 0.15s;
  }
  .btn-ghost:hover:not(:disabled) {
    border-color: var(--t1);
    color: var(--t1);
  }

  .btn-primary {
    background: var(--ac);
    border: none;
    border-radius: 6px;
    padding: 7px 16px;
    font-size: 12px;
    font-weight: 600;
    color: #fff;
    cursor: pointer;
    transition: opacity 0.15s;
  }
  .btn-primary:hover:not(:disabled) {
    opacity: 0.85;
  }
  .btn-primary:disabled,
  .btn-ghost:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
