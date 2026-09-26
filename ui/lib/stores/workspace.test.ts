import { get } from 'svelte/store';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { assignSession, createTab, restoreWorkspace, splitPane, workspace } from './workspace';

describe('workspace tabs', () => {
  const storage = new Map<string, string>();

  beforeEach(() => {
    vi.stubGlobal('localStorage', {
      clear: () => storage.clear(),
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
      removeItem: (key: string) => storage.delete(key),
    });
    localStorage.clear();
    restoreWorkspace(new Set());
  });

  it('opens assigned sessions as active tabs in the focused pane', () => {
    const paneId = get(workspace).focusedPaneId;

    expect(paneId).not.toBeNull();
    assignSession(paneId!, 42);

    const pane = get(workspace).panes[paneId!];
    expect(pane.tabs).toHaveLength(1);
    expect(pane.tabs[0].target).toEqual({ kind: 'agent', sessionId: 42 });
    expect(pane.activeTabId).toBe(pane.tabs[0].id);
  });

  it('closes stale terminal panes when switching chat sessions', () => {
    const chatPaneId = get(workspace).focusedPaneId!;
    assignSession(chatPaneId, 42);
    splitPane(
      chatPaneId,
      'horizontal',
      createTab({ kind: 'terminal', terminalId: 'terminal-1', cwd: '/project' })
    );

    const terminalPaneId = get(workspace).focusedPaneId!;
    assignSession(terminalPaneId, 99);

    const state = get(workspace);
    const chatPane = state.panes[chatPaneId];
    const chatTab = chatPane.tabs.find((tab) => tab.id === chatPane.activeTabId);

    expect(state.panes[terminalPaneId]).toBeUndefined();
    expect(chatTab?.target).toEqual({ kind: 'agent', sessionId: 99 });
  });
});
