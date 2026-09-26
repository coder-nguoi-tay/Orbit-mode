import { writable } from 'svelte/store';

export type ToastType = 'error' | 'warning' | 'info' | 'success' | 'update';

export interface ToastAction {
  label: string;
  onClick: () => void;
}

export interface Toast {
  id: string;
  type: ToastType;
  message: string;
  autoDismiss: boolean;
  action?: ToastAction;
}

export const toasts = writable<Toast[]>([]);

export function addToast(toast: Omit<Toast, 'id'>, duration?: number): string {
  const id = Math.random().toString(36).slice(2);

  toasts.update((list) => {
    // If identical message exists, replace it to avoid clutter
    const filtered = list.filter((t) => t.message !== toast.message);
    return [...filtered, { ...toast, id }];
  });

  if (toast.autoDismiss) {
    const timeout = duration ?? (toast.type === 'success' ? 2200 : 4500);
    setTimeout(() => removeToast(id), timeout);
  }

  return id;
}

export function removeToast(id: string): void {
  toasts.update((list) => list.filter((t) => t.id !== id));
}
