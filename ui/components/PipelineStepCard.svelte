<script lang="ts">
  import type { PipelineStep } from '../lib/types';

  export let step: PipelineStep;

  const roleLabel: Record<string, string> = {
    planner: 'Planner',
    developer: 'Developer',
    reviewer: 'Reviewer',
    tester: 'Tester',
    auditor: 'Auditor',
  };

  const statusIcon: Record<string, string> = {
    pending: '○',
    starting: '◎',
    running: '●',
    waiting: '◌',
    passed: '✓',
    failed: '✗',
    needs_changes: '!',
    cancelled: '—',
    skipped: '·',
  };

  const statusClass: Record<string, string> = {
    pending: 'pending',
    starting: 'running',
    running: 'running',
    waiting: 'running',
    passed: 'passed',
    failed: 'failed',
    needs_changes: 'attention',
    cancelled: 'cancelled',
    skipped: 'cancelled',
  };

  function duration(step: PipelineStep): string {
    if (!step.startedAt) return '';
    const start = new Date(step.startedAt).getTime();
    const end = step.completedAt ? new Date(step.completedAt).getTime() : Date.now();
    const secs = Math.round((end - start) / 1000);
    if (secs < 60) return `${secs}s`;
    return `${Math.floor(secs / 60)}m ${secs % 60}s`;
  }
</script>

<div class="step-card" class:active={step.status === 'running' || step.status === 'starting'}>
  <span class="step-icon {statusClass[step.status] ?? 'pending'}">
    {statusIcon[step.status] ?? '○'}
  </span>
  <div class="step-body">
    <div class="step-header">
      <span class="step-role">{roleLabel[step.role] ?? step.role}</span>
      {#if step.attempt > 1}
        <span class="step-attempt">attempt {step.attempt}</span>
      {/if}
      <span class="step-provider">{step.providerId}</span>
      {#if step.model}
        <span class="step-model">{step.model}</span>
      {/if}
    </div>
    {#if step.status === 'running' || step.status === 'starting'}
      <div class="step-running-bar"></div>
    {/if}
    {#if step.error}
      <div class="step-error">{step.error}</div>
    {/if}
    {#if step.startedAt}
      <div class="step-duration">{duration(step)}</div>
    {/if}
  </div>
</div>

<style>
  .step-card {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 10px;
    border-radius: 6px;
    background: var(--bg2);
    border: 1px solid var(--bd);
    transition: border-color 0.15s;
  }
  .step-card.active {
    border-color: var(--ac);
  }

  .step-icon {
    font-size: 13px;
    flex-shrink: 0;
    width: 16px;
    text-align: center;
    padding-top: 2px;
  }
  .step-icon.pending {
    color: var(--t3);
  }
  .step-icon.running {
    color: var(--ac);
  }
  .step-icon.passed {
    color: var(--success, #4ade80);
  }
  .step-icon.failed {
    color: var(--error, #f87171);
  }
  .step-icon.attention {
    color: var(--warning, #fb923c);
  }
  .step-icon.cancelled {
    color: var(--t3);
  }

  .step-body {
    flex: 1;
    min-width: 0;
  }

  .step-header {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }

  .step-role {
    font-size: var(--sm, 12px);
    font-weight: 600;
    color: var(--t1);
  }

  .step-provider,
  .step-model {
    font-size: var(--xs, 11px);
    color: var(--t3);
    font-family: var(--mono);
  }

  .step-attempt {
    font-size: var(--xs, 11px);
    color: var(--warning, #fb923c);
    font-family: var(--mono);
  }

  .step-duration {
    font-size: var(--xs, 11px);
    color: var(--t3);
    margin-top: 2px;
    font-family: var(--mono);
  }

  .step-error {
    font-size: var(--xs, 11px);
    color: var(--error, #f87171);
    margin-top: 3px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .step-running-bar {
    height: 2px;
    width: 100%;
    margin-top: 6px;
    background: linear-gradient(90deg, var(--ac) 0%, transparent 60%);
    background-size: 200%;
    animation: slide 1.4s linear infinite;
    border-radius: 1px;
  }

  @keyframes slide {
    from {
      background-position: 100% 0;
    }
    to {
      background-position: -100% 0;
    }
  }
</style>
