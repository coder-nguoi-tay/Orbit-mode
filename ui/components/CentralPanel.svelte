<script lang="ts">
  import { get } from 'svelte/store';
  import type { Session } from '../lib/stores/sessions';
  import { journal, pendingMessages } from '../lib/stores/journal';
  import { backends as backendsStore } from '../lib/stores/providers';
  import { getSessionJournal } from '../lib/tauri/sessions';
  import { invoke } from '../lib/tauri/invoke';
  import { updateSessionState, sessions } from '../lib/stores/sessions';
  import { statusColor, statusLabel, modelShortName } from '../lib/status';
  import { shortenPath } from '../lib/path';
  import { metaPanelVisible, compactDensity } from '../lib/stores/preferences';
  import { inspectorToggleHint } from '../lib/shortcuts';
  import Feed from './Feed.svelte';
  import InputBar from './InputBar.svelte';
  import PermissionDialog from './PermissionDialog.svelte';
  import PanelHeader from './workspace/PanelHeader.svelte';

  export let session: Session;
  export let onClose: (() => void) | null = null;
  export let focused: boolean = true;

  // The user's compact-density toggle is the single source of truth. Previously
  // this was OR-ed with a per-pane auto hint, which forced compact "on" whenever
  // a pane was split or had multiple tabs — so the toggle could never turn it off.
  $: effectiveCompact = $compactDensity;

  let feedComponent: Feed;
  let atBottom = true;

  /** Load one session's journal once and cache empty histories as well.
   * @param id Session whose history is being opened.
   * @return Completion after the journal cache has been checked or populated.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-26
   */
  async function loadHistory(id: number): Promise<void> {
    if (get(journal).has(id)) return;
    try {
      const entries = await getSessionJournal(id);
      journal.update((m) => (m.has(id) ? m : new Map(m).set(id, entries)));
    } catch (_e) {
      /* no-op */
    }
  }

  /** Read a session's Git branch only when the session has no cached branch.
   * @return Completion after the branch is read or the repository is skipped.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-26
   */
  async function fetchBranch(): Promise<void> {
    if (!session.cwd || session.gitBranch) return;
    try {
      const branch = await invoke<string | null>('git_branch', { cwd: session.cwd });
      if (branch && (session.gitBranch ?? null) !== branch) {
        sessions.update((l) => updateSessionState(l, session.id, { gitBranch: branch }));
      }
    } catch {
      /* not a git repo — no-op */
    }
  }

  let loadedId: number | null = null;
  $: if (session?.id != null && session.id !== loadedId) {
    loadedId = session.id;
    loadHistory(session.id);
    fetchBranch();
  }

  $: entries = $journal.get(session?.id) ?? [];

  // Clear pending messages only when a NEW entry arrives (entries grew).
  let prevEntryCount = 0;
  $: {
    const count = entries.length;
    if (count > prevEntryCount) {
      const last = entries[count - 1];
      if (
        last &&
        (last.entryType === 'user' ||
          last.entryType === 'assistant' ||
          last.entryType === 'toolCall')
      ) {
        pendingMessages.clear();
      }
    }
    prevEntryCount = count;
  }

  function scrollToBottom() {
    feedComponent?.scrollToBottom();
    atBottom = true;
  }

  $: statusStr = statusLabel(session?.status ?? '');
  $: statusClr = statusColor(session?.status ?? '');

  $: topbarBranch = session?.branchName ?? session?.gitBranch ?? null;
  $: topbarPathFull = session?.cwd ?? null;
  $: topbarPath = topbarPathFull ? shortenPath(topbarPathFull) : null;

  function fmtModel(m: string | null): string {
    return modelShortName(m);
  }

  function parseToolName(approval: string): string {
    const match = approval.match(/^Allow\s+(.+?)\?/);
    return match ? match[1] : approval;
  }

  function parseToolDesc(approval: string): string {
    const match = approval.match(/^Allow\s+.+?\?\s*(.*)/);
    return match ? match[1].trim() : '';
  }

  $: providerModelIds = (() => {
    const p = session?.provider ?? 'claude-code';
    for (const b of $backendsStore) {
      if (b.id === p) return b.models.map((m) => m.id);
      const sub = b.subProviders?.find((s) => s.id === p);
      if (sub) return sub.models.map((m) => m.id);
    }
    return [];
  })();
</script>

<div class="panel">
  <PanelHeader
    title={session.name ??
      session.projectName ??
      session.cwd?.split(/[\\/]/).pop() ??
      `#${session.id}`}
    branch={topbarBranch}
    path={topbarPath}
    pathFull={topbarPathFull}
    status={statusStr}
    model={fmtModel(session.model)}
    contextPercent={session.contextPercent}
    statusColor={statusClr}
    {onClose}
    {focused}
  />

  <!-- Inspector badge (hidden when meta panel is visible) -->
  {#if !$metaPanelVisible}
    <div
      class="inspector-pop"
      role="button"
      tabindex="0"
      on:click={() => metaPanelVisible.set(true)}
      on:keydown={(e) => e.key === 'Enter' && metaPanelVisible.set(true)}
      title="Toggle inspector panel"
    >
      inspector hidden • {inspectorToggleHint()}
    </div>
  {/if}

  {#if session.pendingApproval}
    <PermissionDialog
      sessionId={session.id}
      toolName={parseToolName(session.pendingApproval)}
      description={parseToolDesc(session.pendingApproval)}
    />
  {/if}

  <!-- Feed -->
  <div class="feed-wrap" data-testid="session-feed">
    {#if entries.length === 0 && $pendingMessages.length === 0}
      <div class="feed-empty">
        <span>session #{session.id} · {statusStr}</span>
      </div>
    {:else}
      {#key session.id}
        <Feed
          bind:this={feedComponent}
          {entries}
          status={session.status}
          provider={session.provider ?? 'claude-code'}
          cwd={session.cwd}
          compact={effectiveCompact}
          on:bottomchange={(e) => (atBottom = e.detail.atBottom)}
        />
      {/key}
      {#each $pendingMessages as msg (msg.id)}
        <div class="pending-msg">
          <span class="pending-arrow">›</span>
          <div class="pending-body">
            <div class="pending-meta">
              <span class="pending-tag">YOU</span>
              <span class="pending-status">sending...</span>
            </div>
            <span class="pending-text">{msg.text}</span>
          </div>
        </div>
      {/each}
    {/if}

    {#if !atBottom}
      <button class="scroll-btn" on:click={scrollToBottom}>↓ scroll to bottom</button>
    {/if}
  </div>

  <!-- Input -->
  <InputBar
    sessionId={session.id}
    cwd={session.cwd ?? ''}
    sessionStatus={session.status}
    provider={session.provider}
    providerModels={providerModelIds}
    compact={effectiveCompact}
  />
</div>

<style>
  .panel {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--bg);
  }

  /* ── Inspector pop badge ── */
  .inspector-pop {
    position: absolute;
    top: 44px;
    right: 24px;
    display: flex;
    gap: 8px;
    align-items: center;
    color: var(--t3);
    font-family: var(--mono);
    font-size: 10px;
    border: 1px solid var(--bd);
    background: color-mix(in srgb, var(--t0), transparent 97%);
    border-radius: 999px;
    padding: 5px 10px;
    cursor: pointer;
    z-index: 5;
    transition: all 0.15s;
  }
  .inspector-pop:hover {
    border-color: var(--t2);
    color: var(--t1);
    background: color-mix(in srgb, var(--t0), transparent 94%);
  }

  .feed-wrap {
    flex: 1;
    overflow: hidden;
    min-height: 0;
    display: flex;
    flex-direction: column;
    position: relative;
  }

  .feed-empty {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    font-size: var(--sm);
    color: var(--t3);
  }

  .pending-msg {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    padding: 10px 14px;
    margin: 8px 24px;
    background: linear-gradient(135deg, rgba(79, 146, 247, 0.08) 0%, rgba(79, 146, 247, 0.02) 100%);
    border: 1px solid rgba(79, 146, 247, 0.22);
    border-left: 3px solid var(--user-fg, #4f92f7);
    border-radius: 6px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.2), inset 0 1px 0 rgba(255, 255, 255, 0.04);
    font-family: var(--font-mono, var(--mono), monospace);
    animation: pendingPulse 2.5s ease-in-out infinite;
  }
  .pending-arrow {
    color: var(--user-fg, #4f92f7);
    font-weight: 700;
    font-size: 15px;
    line-height: 1.2;
    flex-shrink: 0;
    margin-top: 2px;
    text-shadow: 0 0 8px rgba(79, 146, 247, 0.5);
  }
  .pending-body {
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: 1;
    min-width: 0;
  }
  .pending-meta {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .pending-tag {
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.06em;
    color: var(--user-fg, #4f92f7);
    background: rgba(79, 146, 247, 0.14);
    border: 1px solid rgba(79, 146, 247, 0.28);
    border-radius: 4px;
    padding: 1px 5px;
  }
  .pending-status {
    font-size: 9.5px;
    font-family: var(--mono);
    color: var(--t2, #8b9991);
    opacity: 0.7;
    letter-spacing: 0.02em;
  }
  .pending-text {
    font-size: 13px;
    line-height: 1.55;
    color: var(--t0, #ffffff);
    white-space: pre-wrap;
    word-break: break-word;
    font-weight: 500;
  }
  @keyframes pendingPulse {
    0%, 100% {
      border-color: rgba(79, 146, 247, 0.22);
      border-left-color: var(--user-fg, #4f92f7);
    }
    50% {
      border-color: rgba(79, 146, 247, 0.45);
      border-left-color: #70a7ff;
      box-shadow: 0 4px 18px rgba(0, 0, 0, 0.25), 0 0 14px rgba(79, 146, 247, 0.1);
    }
  }

  .scroll-btn {
    position: absolute;
    bottom: 14px;
    right: 38px;
    z-index: 10;
    background: var(--bg2);
    border: 1px solid var(--bd1);
    border-radius: 999px;
    color: var(--t1);
    font-size: var(--xs);
    padding: var(--sp-3) var(--sp-6);
    box-shadow: 0 14px 34px rgba(0, 0, 0, 0.28);
  }
  .scroll-btn:hover {
    border-color: var(--ac);
    color: var(--ac);
  }

  @media (max-width: 768px) {
    .scroll-btn {
      right: 18px;
      bottom: 10px;
    }
  }
</style>
