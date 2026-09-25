<script lang="ts">
  import type { Pipeline } from '../lib/types';
  import PipelineStepCard from './PipelineStepCard.svelte';
  import { cancelPipeline } from '../lib/tauri/pipelines';
  import { updatePipelineStatus, activePipelineId } from '../lib/stores/pipelines';
  import { assignSession, workspace } from '../lib/stores/workspace';
  import { get } from 'svelte/store';

  function viewSession(sessionId: number) {
    activePipelineId.set(null);
    const ws = get(workspace);
    if (ws.focusedPaneId) assignSession(ws.focusedPaneId, sessionId);
  }

  export let pipeline: Pipeline;

  const statusLabel: Record<string, string> = {
    created: 'Created',
    preflight: 'Preflight',
    planning: 'Planning',
    plan_ready: 'Plan Ready',
    implementing: 'Implementing',
    reviewing: 'Reviewing',
    changes_requested: 'Changes Requested',
    testing: 'Testing',
    test_failed: 'Test Failed',
    final_review: 'Final Review',
    quality_gate: 'Quality Gate',
    ready_for_human: 'Ready for Review',
    completed: 'Completed',
    paused: 'Paused',
    failed: 'Failed',
    cancelled: 'Cancelled',
  };

  const terminalStatuses = new Set(['completed', 'failed', 'cancelled']);
  $: isTerminal = terminalStatuses.has(pipeline.status);

  async function handleCancel() {
    try {
      const updated = await cancelPipeline(pipeline.id);
      updatePipelineStatus(updated.id, updated.status, updated.steps);
    } catch (e) {
      console.error('cancel pipeline failed', e);
    }
  }
</script>

<div class="pipeline-panel">
  <header class="pipeline-header">
    <div class="pipeline-title">
      <span class="pipeline-name">{pipeline.name}</span>
      <span class="pipeline-status status-{pipeline.status}">
        {statusLabel[pipeline.status] ?? pipeline.status}
      </span>
    </div>
    <div class="pipeline-actions">
      {#if !isTerminal}
        <button class="btn-ghost danger" on:click={handleCancel}>Cancel</button>
      {/if}
    </div>
  </header>

  <div class="pipeline-request">
    <div class="request-label">Task</div>
    <div class="request-text">{pipeline.userRequest}</div>
  </div>

  <div class="pipeline-steps">
    <div class="steps-label">Steps</div>
    {#each pipeline.steps as step (step.id)}
      <div class="step-row">
        <PipelineStepCard {step} />
        {#if step.sessionId}
          <button class="btn-view" on:click={() => viewSession(step.sessionId!)}>View</button>
        {/if}
      </div>
    {/each}
    {#if pipeline.steps.length === 0}
      <div class="steps-empty">No steps yet</div>
    {/if}
  </div>

  {#if pipeline.reviewLoops > 0 || pipeline.testLoops > 0}
    <div class="pipeline-loops">
      {#if pipeline.reviewLoops > 0}
        <span class="loop-badge">Review loops: {pipeline.reviewLoops}</span>
      {/if}
      {#if pipeline.testLoops > 0}
        <span class="loop-badge">Test loops: {pipeline.testLoops}</span>
      {/if}
    </div>
  {/if}

  <div class="pipeline-meta">
    <span class="meta-item">Created {new Date(pipeline.createdAt).toLocaleString()}</span>
    {#if pipeline.worktreePath}
      <span class="meta-item path">{pipeline.worktreePath}</span>
    {/if}
  </div>
</div>

<style>
  .pipeline-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    padding: 20px 24px;
    gap: 20px;
    overflow-y: auto;
    background: var(--bg);
  }

  .pipeline-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }

  .pipeline-title {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .pipeline-name {
    font-size: 16px;
    font-weight: 600;
    color: var(--t0);
  }

  .pipeline-status {
    font-size: 11px;
    font-family: var(--mono);
    padding: 2px 8px;
    border-radius: 999px;
    border: 1px solid var(--bd);
    color: var(--t2);
  }
  .pipeline-status.status-running,
  .pipeline-status.status-planning,
  .pipeline-status.status-implementing,
  .pipeline-status.status-reviewing,
  .pipeline-status.status-testing,
  .pipeline-status.status-final_review,
  .pipeline-status.status-quality_gate,
  .pipeline-status.status-preflight {
    color: var(--ac);
    border-color: var(--ac);
  }
  .pipeline-status.status-completed,
  .pipeline-status.status-ready_for_human {
    color: var(--success, #4ade80);
    border-color: var(--success, #4ade80);
  }
  .pipeline-status.status-failed,
  .pipeline-status.status-cancelled {
    color: var(--error, #f87171);
    border-color: var(--error, #f87171);
  }
  .pipeline-status.status-paused,
  .pipeline-status.status-changes_requested,
  .pipeline-status.status-test_failed {
    color: var(--warning, #fb923c);
    border-color: var(--warning, #fb923c);
  }

  .pipeline-request {
    background: var(--bg2);
    border: 1px solid var(--bd);
    border-radius: 8px;
    padding: 12px 14px;
  }
  .request-label {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--t3);
    margin-bottom: 6px;
  }
  .request-text {
    font-size: var(--sm, 12px);
    color: var(--t1);
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .pipeline-steps {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .steps-label {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--t3);
    margin-bottom: 2px;
  }
  .steps-empty {
    font-size: var(--sm, 12px);
    color: var(--t3);
    padding: 8px 0;
  }

  .step-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .step-row > :global(.step-card) {
    flex: 1;
  }
  .btn-view {
    flex-shrink: 0;
    background: transparent;
    border: 1px solid var(--bd);
    border-radius: 5px;
    padding: 3px 8px;
    font-size: 11px;
    color: var(--t3);
    cursor: pointer;
    transition: all 0.15s;
    white-space: nowrap;
  }
  .btn-view:hover {
    border-color: var(--ac);
    color: var(--ac);
  }

  .pipeline-loops {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .loop-badge {
    font-size: 11px;
    font-family: var(--mono);
    color: var(--t2);
    background: var(--bg2);
    border: 1px solid var(--bd);
    border-radius: 999px;
    padding: 2px 8px;
  }

  .pipeline-meta {
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin-top: auto;
  }
  .meta-item {
    font-size: 11px;
    color: var(--t3);
    font-family: var(--mono);
  }
  .meta-item.path {
    word-break: break-all;
  }

  .pipeline-actions {
    display: flex;
    gap: 8px;
  }

  .btn-ghost {
    background: transparent;
    border: 1px solid var(--bd);
    border-radius: 5px;
    padding: 4px 10px;
    font-size: 12px;
    color: var(--t2);
    cursor: pointer;
    transition: all 0.15s;
  }
  .btn-ghost:hover {
    border-color: var(--t1);
    color: var(--t1);
  }
  .btn-ghost.danger:hover {
    border-color: var(--error, #f87171);
    color: var(--error, #f87171);
  }
</style>
