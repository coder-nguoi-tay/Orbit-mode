<script lang="ts">
  import '../app.css';
  import { onMount } from 'svelte';
  import type { Snippet } from 'svelte';
  import { AlertTriangle, X, Copy, Check, RefreshCw } from 'lucide-svelte';

  let { children }: { children: Snippet } = $props();

  let uncaughtError = $state<{
    message: string;
    stack?: string;
    source?: string;
    lineno?: number;
    colno?: number;
  } | null>(null);

  let copied = $state(false);

  // Disable browser native context menu globally — we handle it ourselves per-element
  function preventNativeContextMenu(e: MouseEvent) {
    e.preventDefault();
  }

  function copyError() {
    if (!uncaughtError) return;
    const text = `Error: ${uncaughtError.message}\nSource: ${uncaughtError.source}:${uncaughtError.lineno}:${uncaughtError.colno}\n\nStack:\n${uncaughtError.stack ?? ''}`;
    navigator.clipboard.writeText(text);
    copied = true;
    setTimeout(() => (copied = false), 2000);
  }

  onMount(() => {
    const errorHandler = (event: ErrorEvent) => {
      console.error('[Orbit-mode Debug Catcher] Error:', event);
      uncaughtError = {
        message: event.message || String(event.error),
        stack: event.error?.stack,
        source: event.filename,
        lineno: event.lineno,
        colno: event.colno,
      };
    };

    const rejectionHandler = (event: PromiseRejectionEvent) => {
      const reason = event.reason;
      const msg = reason instanceof Error ? reason.message : String(reason);
      // Monaco worker sends these for languages without a dedicated language worker — harmless
      if (msg.startsWith('Missing requestHandler or method:')) {
        event.preventDefault();
        return;
      }
      console.error('[Orbit-mode Debug Catcher] Unhandled Rejection:', event);
      uncaughtError = {
        message: msg,
        stack: reason instanceof Error ? reason.stack : undefined,
      };
    };

    window.addEventListener('error', errorHandler);
    window.addEventListener('unhandledrejection', rejectionHandler);

    return () => {
      window.removeEventListener('error', errorHandler);
      window.removeEventListener('unhandledrejection', rejectionHandler);
    };
  });
</script>

<svelte:window on:contextmenu={preventNativeContextMenu} />

<div id="app">
  {@render children()}
</div>

{#if uncaughtError}
  <div class="runtime-error-overlay" role="dialog" aria-modal="true">
    <div class="runtime-error-box">
      <div class="error-box-header">
        <div class="error-title-row">
          <AlertTriangle size={18} class="warn-icon" />
          <span class="error-heading">Runtime Error Captured</span>
        </div>
        <button class="close-err-btn" onclick={() => (uncaughtError = null)} aria-label="Dismiss">
          <X size={15} />
        </button>
      </div>

      <div class="error-msg">{uncaughtError.message}</div>

      {#if uncaughtError.source}
        <div class="error-loc">
          {uncaughtError.source}:{uncaughtError.lineno}:{uncaughtError.colno}
        </div>
      {/if}

      {#if uncaughtError.stack}
        <pre class="error-stack">{uncaughtError.stack}</pre>
      {/if}

      <div class="error-box-actions">
        <button class="err-btn secondary" onclick={copyError}>
          {#if copied}
            <Check size={13} />
            <span>Copied</span>
          {:else}
            <Copy size={13} />
            <span>Copy Error Details</span>
          {/if}
        </button>
        <button class="err-btn primary" onclick={() => window.location.reload()}>
          <RefreshCw size={13} />
          <span>Reload</span>
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .runtime-error-overlay {
    position: fixed;
    bottom: 24px;
    right: 24px;
    z-index: 99999;
    max-width: 600px;
    width: calc(100vw - 48px);
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.7);
  }
  .runtime-error-box {
    background: #141716;
    border: 1px solid #f04848;
    border-radius: 8px;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    color: #e6ede9;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
  }
  .error-box-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .error-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  :global(.warn-icon) {
    color: #f04848;
  }
  .error-heading {
    font-size: 13px;
    font-weight: 600;
    color: #f04848;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .close-err-btn {
    background: none;
    border: none;
    color: #8b9991;
    cursor: pointer;
    padding: 2px;
    border-radius: 4px;
    display: flex;
    align-items: center;
  }
  .close-err-btn:hover {
    color: #ffffff;
    background: #1d2520;
  }
  .error-msg {
    font-size: 13px;
    font-weight: 500;
    color: #ffffff;
    word-break: break-word;
  }
  .error-loc {
    font-family: ui-monospace, monospace;
    font-size: 11px;
    color: #8b9991;
    word-break: break-all;
  }
  .error-stack {
    background: #0a0b0b;
    border: 1px solid #1e2421;
    border-radius: 4px;
    padding: 8px 10px;
    font-family: ui-monospace, monospace;
    font-size: 10px;
    color: #f08c8c;
    overflow-x: auto;
    white-space: pre-wrap;
    max-height: 180px;
    margin: 0;
  }
  .error-box-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 4px;
  }
  .err-btn {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 5px 10px;
    border-radius: 4px;
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s;
    border: none;
  }
  .err-btn.primary {
    background: #f04848;
    color: #ffffff;
  }
  .err-btn.primary:hover {
    background: #ff5c5c;
  }
  .err-btn.secondary {
    background: #1d2520;
    color: #c5d1cb;
    border: 1px solid #2a332e;
  }
  .err-btn.secondary:hover {
    color: #ffffff;
    border-color: #3b4740;
  }
</style>
