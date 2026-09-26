<script lang="ts">
  import type { Toast } from '../lib/stores/toasts';
  import { removeToast } from '../lib/stores/toasts';
  import { Check, AlertCircle, AlertTriangle, Info, ArrowUpCircle, X } from 'lucide-svelte';

  export let toast: Toast;
</script>

<div class="toast toast--{toast.type}" role="alert">
  <div class="toast-icon-wrap">
    {#if toast.type === 'success'}
      <Check size={13} strokeWidth={2.5} />
    {:else if toast.type === 'error'}
      <AlertCircle size={13} strokeWidth={2.2} />
    {:else if toast.type === 'warning'}
      <AlertTriangle size={13} strokeWidth={2.2} />
    {:else if toast.type === 'update'}
      <ArrowUpCircle size={13} strokeWidth={2.2} />
    {:else}
      <Info size={13} strokeWidth={2.2} />
    {/if}
  </div>

  <div class="toast-body">
    <span class="toast-message">{toast.message}</span>
    {#if toast.action}
      <button
        class="toast-action"
        on:click={() => {
          toast.action?.onClick();
          removeToast(toast.id);
        }}
      >
        {toast.action.label}
      </button>
    {/if}
  </div>

  <button class="toast-close" on:click={() => removeToast(toast.id)} aria-label="Dismiss">
    <X size={12} />
  </button>
</div>

<style>
  .toast {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px 6px 8px;
    border-radius: 999px;
    background: rgba(13, 14, 16, 0.88);
    backdrop-filter: blur(14px);
    -webkit-backdrop-filter: blur(14px);
    border: 1px solid rgba(255, 255, 255, 0.09);
    box-shadow: 0 4px 20px -2px rgba(0, 0, 0, 0.5), 0 0 0 1px rgba(0, 0, 0, 0.4);
    min-width: 0;
    max-width: 380px;
    pointer-events: all;
    user-select: none;
    animation: toastPop 0.22s cubic-bezier(0.16, 1, 0.3, 1);
    transition: transform 0.15s ease, opacity 0.15s ease;
  }

  @keyframes toastPop {
    from {
      opacity: 0;
      transform: translateY(-8px) scale(0.94);
    }
    to {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
  }

  .toast-icon-wrap {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  /* ── Variants ── */
  .toast--success {
    border-color: rgba(124, 255, 158, 0.22);
    box-shadow: 0 4px 18px -2px rgba(0, 0, 0, 0.5), 0 0 12px rgba(124, 255, 158, 0.08);
  }
  .toast--success .toast-icon-wrap {
    background: rgba(124, 255, 158, 0.15);
    color: var(--ac);
  }

  .toast--error {
    border-color: rgba(228, 119, 112, 0.3);
    box-shadow: 0 4px 18px -2px rgba(0, 0, 0, 0.5), 0 0 12px rgba(228, 119, 112, 0.1);
  }
  .toast--error .toast-icon-wrap {
    background: rgba(228, 119, 112, 0.16);
    color: var(--s-error);
  }

  .toast--warning {
    border-color: rgba(242, 201, 111, 0.25);
  }
  .toast--warning .toast-icon-wrap {
    background: rgba(242, 201, 111, 0.15);
    color: var(--s-input);
  }

  .toast--info {
    border-color: rgba(77, 163, 255, 0.25);
  }
  .toast--info .toast-icon-wrap {
    background: rgba(77, 163, 255, 0.15);
    color: var(--s-init);
  }

  .toast--update {
    border-color: rgba(124, 255, 158, 0.35);
    box-shadow: 0 4px 20px -2px rgba(0, 0, 0, 0.5), 0 0 16px rgba(124, 255, 158, 0.15);
  }
  .toast--update .toast-icon-wrap {
    background: var(--ac);
    color: #000;
  }

  .toast-body {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    flex: 1;
  }

  .toast-message {
    font-size: 11.5px;
    font-weight: 500;
    color: var(--t0);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    line-height: 1.3;
  }

  .toast-action {
    background: var(--ac);
    color: #000;
    border: none;
    border-radius: 999px;
    padding: 2px 8px;
    font-size: 10px;
    font-weight: 600;
    cursor: pointer;
    flex-shrink: 0;
    transition: filter 0.12s;
  }

  .toast-action:hover {
    filter: brightness(1.1);
  }

  .toast-close {
    background: transparent;
    border: none;
    color: var(--t3);
    padding: 2px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    flex-shrink: 0;
    transition: color 0.12s, background 0.12s;
  }

  .toast-close:hover {
    color: var(--t0);
    background: rgba(255, 255, 255, 0.1);
  }
</style>
