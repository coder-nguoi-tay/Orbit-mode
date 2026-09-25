<script lang="ts">
  import { page } from '$app/stores';
  import { AlertTriangle, RefreshCw, Copy, Check } from 'lucide-svelte';

  let copied = false;

  function copyError() {
    const text = `${$page.status} ${$page.error?.message ?? 'Internal Error'}\n\n${($page.error as any)?.stack ?? ''}`;
    navigator.clipboard.writeText(text);
    copied = true;
    setTimeout(() => (copied = false), 2000);
  }

  function reload() {
    window.location.reload();
  }
</script>

<div class="error-container">
  <div class="error-card">
    <div class="error-header">
      <div class="error-icon">
        <AlertTriangle size={24} />
      </div>
      <div>
        <div class="error-status">ERROR {$page.status}</div>
        <h1 class="error-title">{$page.error?.message ?? 'Internal Error'}</h1>
      </div>
    </div>

    {#if ($page.error as any)?.stack}
      <div class="stack-section">
        <div class="stack-label">Stack Trace:</div>
        <pre class="stack-trace">{($page.error as any).stack}</pre>
      </div>
    {/if}

    <div class="error-actions">
      <button class="btn primary" on:click={reload}>
        <RefreshCw size={14} />
        <span>Reload Page</span>
      </button>
      <button class="btn secondary" on:click={copyError}>
        {#if copied}
          <Check size={14} />
          <span>Copied Error Details</span>
        {:else}
          <Copy size={14} />
          <span>Copy Error Details</span>
        {/if}
      </button>
    </div>
  </div>
</div>

<style>
  .error-container {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 100vh;
    padding: 24px;
    background: #0c0d0d;
    color: #e6ede9;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
  }
  .error-card {
    background: #141716;
    border: 1px solid #2a332e;
    border-radius: 8px;
    padding: 28px;
    max-width: 720px;
    width: 100%;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.6);
    display: flex;
    flex-direction: column;
    gap: 20px;
  }
  .error-header {
    display: flex;
    align-items: flex-start;
    gap: 16px;
  }
  .error-icon {
    width: 44px;
    height: 44px;
    border-radius: 8px;
    background: rgba(240, 72, 72, 0.12);
    color: #f04848;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }
  .error-status {
    font-size: 11px;
    font-weight: 600;
    color: #f04848;
    letter-spacing: 0.08em;
    font-family: ui-monospace, monospace;
    margin-bottom: 4px;
  }
  .error-title {
    font-size: 16px;
    font-weight: 600;
    color: #ffffff;
    margin: 0;
    line-height: 1.4;
    word-break: break-word;
  }
  .stack-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .stack-label {
    font-size: 11px;
    color: #8b9991;
    font-family: ui-monospace, monospace;
  }
  .stack-trace {
    background: #0a0b0b;
    border: 1px solid #1e2421;
    border-radius: 6px;
    padding: 12px;
    font-family: ui-monospace, monospace;
    font-size: 11px;
    color: #f08c8c;
    overflow-x: auto;
    white-space: pre-wrap;
    max-height: 280px;
    margin: 0;
  }
  .error-actions {
    display: flex;
    gap: 12px;
  }
  .btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 14px;
    border-radius: 6px;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s;
    border: none;
  }
  .btn.primary {
    background: #00d47e;
    color: #0c0d0d;
  }
  .btn.primary:hover {
    background: #00ea8b;
  }
  .btn.secondary {
    background: #1d2520;
    color: #c5d1cb;
    border: 1px solid #2a332e;
  }
  .btn.secondary:hover {
    color: #ffffff;
    border-color: #3b4740;
  }
</style>
