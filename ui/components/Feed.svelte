<script lang="ts">
  import { onMount, onDestroy, tick, createEventDispatcher } from 'svelte';
  import type { JournalEntry } from '../lib/types';
  import Markdown from './Markdown.svelte';
  import ToolCallEntry from './ToolCallEntry.svelte';
  import { backends } from '../lib/stores/providers';

  export let entries: JournalEntry[] = [];
  export let status: string = '';
  export let provider: string = 'claude-code';
  export let cwd: string | null = null;
  export let compact = false;
  export let hasEarlierEntries = false;
  export let loadingEarlierEntries = false;
  export let onLoadEarlier: (() => Promise<void>) | null = null;

  $: agentLabel = (() => {
    const direct = $backends.find((b) => b.id === provider);
    if (direct) return direct.name.toLowerCase();
    const parent = $backends.find((b) => b.subProviders?.some((s) => s.id === provider));
    return parent?.name.toLowerCase() ?? provider;
  })();

  const dispatch = createEventDispatcher<{ bottomchange: { atBottom: boolean } }>();

  $: isWorking =
    status === 'working' ||
    status === 'input' ||
    (provider === 'claude-code' && status === 'running');

  let workingElapsedSeconds = 0;
  let workingStartedAt = 0;
  let workingTimer: ReturnType<typeof setInterval> | null = null;
  let feedMounted = false;
  let knownEntryCount = entries.length;
  let knownLastEntryIdentity = entries.length > 0 ? entryIdentity(entries[entries.length - 1]) : '';
  let streamingAssistantIdentity = '';

  /** Build a stable identity for detecting entries appended after initial history load.
   * @param entry Journal entry whose provider sequence identifies it within this feed.
   * @return Stable epoch, sequence and type key.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-27
   */
  function entryIdentity(entry: JournalEntry): string {
    return `${entry.epoch}:${entry.seq}:${entry.entryType}`;
  }

  /** Mark only newly appended assistant responses for progressive text reveal.
   * @param currentEntries Current journal entries after a store update.
   * @return No value; prepended history and initial history never animate.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-27
   */
  function trackStreamingAssistant(currentEntries: JournalEntry[]): void {
    const previousLastEntryStillAtBoundary =
      knownEntryCount === 0 ||
      (currentEntries[knownEntryCount - 1] != null &&
        entryIdentity(currentEntries[knownEntryCount - 1]) === knownLastEntryIdentity);

    if (
      feedMounted &&
      currentEntries.length > knownEntryCount &&
      previousLastEntryStillAtBoundary
    ) {
      const newAssistantEntry = currentEntries
        .slice(knownEntryCount)
        .reverse()
        .find((entry) => entry.entryType === 'assistant' && entry.text);
      if (newAssistantEntry) streamingAssistantIdentity = entryIdentity(newAssistantEntry);
    }

    knownEntryCount = currentEntries.length;
    knownLastEntryIdentity = currentEntries.length
      ? entryIdentity(currentEntries[currentEntries.length - 1])
      : '';
  }

  /** Start or stop the lightweight elapsed timer shown beside the working state.
   * @param working Whether the provider is currently processing the conversation.
   * @return No value; at most one one-second timer remains active.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-27
   */
  function syncWorkingClock(working: boolean): void {
    if (working && workingTimer === null) {
      workingStartedAt = Date.now();
      workingElapsedSeconds = 0;
      workingTimer = setInterval(() => {
        workingElapsedSeconds = Math.floor((Date.now() - workingStartedAt) / 1000);
      }, 1000);
    } else if (!working && workingTimer !== null) {
      clearInterval(workingTimer);
      workingTimer = null;
      workingElapsedSeconds = 0;
    }
  }

  /** Release the working-state timer when this feed is destroyed.
   * @return No value; timer state is reset.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-27
   */
  function stopWorkingClock(): void {
    if (workingTimer !== null) clearInterval(workingTimer);
    workingTimer = null;
  }

  $: trackStreamingAssistant(entries);
  $: syncWorkingClock(isWorking);

  // ── Display item grouping ──────────────────────────────────────────────────
  interface DisplayItem {
    entry: JournalEntry;
    result: JournalEntry | null;
    streaming: JournalEntry[];
  }

  $: display = (() => {
    const items: DisplayItem[] = [];
    const skip = new Set<number>();
    for (let i = 0; i < entries.length; i++) {
      if (skip.has(i)) continue;
      const e = entries[i];
      if (e.entryType === 'toolCall') {
        const streaming: JournalEntry[] = [];
        let result: JournalEntry | null = null;
        for (let j = i + 1; j < entries.length; j++) {
          if (entries[j].entryType === 'toolResult') {
            result = entries[j];
            skip.add(j);
            break;
          } else if (entries[j].entryType === 'progress') {
            streaming.push(entries[j]);
            skip.add(j);
          } else {
            break;
          }
        }
        items.push({ entry: e, result, streaming });
      } else if (e.entryType === 'toolResult' || e.entryType === 'progress') {
        // handled above
      } else {
        items.push({ entry: e, result: null, streaming: [] });
      }
    }
    return items;
  })();

  // Collapse long runs of the same repeated tool (e.g. 8 reads in a row) into
  // one expandable cluster so they don't bury the conversation.
  const GROUP_MIN = 4;
  type Row =
    | { kind: 'item'; item: DisplayItem }
    | { kind: 'group'; tool: string; items: DisplayItem[] };

  $: rows = (() => {
    const out: Row[] = [];
    let i = 0;
    while (i < display.length) {
      const it = display[i];
      const tool = it.entry.entryType === 'toolCall' ? (it.entry.tool ?? '') : null;
      if (tool) {
        let j = i + 1;
        while (
          j < display.length &&
          display[j].entry.entryType === 'toolCall' &&
          (display[j].entry.tool ?? '') === tool
        ) {
          j++;
        }
        const run = display.slice(i, j);
        if (run.length >= GROUP_MIN) {
          out.push({ kind: 'group', tool, items: run });
          i = j;
          continue;
        }
      }
      out.push({ kind: 'item', item: it });
      i++;
    }
    return out;
  })();

  let expandedGroups = new Set<number>();
  function toggleGroup(absIdx: number) {
    const next = new Set(expandedGroups);
    if (next.has(absIdx)) next.delete(absIdx);
    else next.add(absIdx);
    expandedGroups = next;
  }

  function ts(entry: JournalEntry) {
    return entry.timestamp?.slice(11, 16) ?? '';
  }

  function eventClass(entry: JournalEntry): string {
    if (entry.entryType === 'toolCall') return 'tool';
    if (entry.entryType === 'toolResult' || entry.entryType === 'progress') return 'tool-detail';
    if (entry.entryType === 'assistant') return 'assistant';
    if (entry.entryType === 'user') return 'user';
    return 'system';
  }

  function actorLabel(entry: JournalEntry): string {
    if (entry.entryType === 'user') return 'you';
    if (entry.entryType === 'assistant') return `Orbit-mode / ${agentLabel}`;
    if (entry.entryType === 'toolCall') return 'tool';
    return entry.entryType;
  }

  let expandedThinking = new Set<number>();
  function toggleThinking(i: number) {
    const next = new Set(expandedThinking);
    if (next.has(i)) next.delete(i);
    else next.add(i);
    expandedThinking = next;
  }

  // ── Chunk-based loading ────────────────────────────────────────────────────
  // Render the last PAGE_SIZE items. When the user scrolls to the top,
  // prepend another chunk. Normal browser scroll — no spacers, no height cache.
  const PAGE_SIZE = 200;

  let visibleFrom = 0; // index into display[] from which we render
  let isAtBottom = true;
  let lastScrollTop = 0;
  let scrollerEl: HTMLDivElement;
  let timelineEl: HTMLDivElement;
  let programmaticScroll = false; // flag to ignore onScroll after programmatic changes

  // When display grows, reset visibleFrom to show the tail if at bottom.
  // When display shrinks (session switch via {#key}), always reset.
  let prevTotal = 0;
  $: {
    const total = rows.length;
    if (total < prevTotal) {
      // session remount or reset — start at tail
      visibleFrom = Math.max(0, total - PAGE_SIZE);
    } else if (total > prevTotal && isAtBottom) {
      // new entries arrived while at bottom — keep showing tail
      visibleFrom = Math.max(0, total - PAGE_SIZE);
    }
    prevTotal = total;
  }

  $: visibleItems = rows.slice(visibleFrom);

  $: hasMore = visibleFrom > 0 || hasEarlierEntries;

  // ── Scroll handling ────────────────────────────────────────────────────────
  /** Update follow mode and request older history when the viewport reaches the top.
   * @return No value; scroll state and history loading are updated through component events.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-27
   */
  function onScroll(): void {
    if (!scrollerEl) return;
    const { scrollTop, scrollHeight, clientHeight } = scrollerEl;

    // Skip follow-mode logic for programmatic scrolls (ResizeObserver, scrollToBottom).
    if (programmaticScroll) {
      programmaticScroll = false;
      lastScrollTop = scrollTop;
      return;
    }

    const atBottom = scrollHeight - scrollTop - clientHeight < 80;

    if (atBottom) {
      if (!isAtBottom) {
        isAtBottom = true;
        dispatch('bottomchange', { atBottom: true });
      }
    } else if (scrollTop < lastScrollTop) {
      // User scrolled UP intentionally — stop following.
      if (isAtBottom) {
        isAtBottom = false;
        dispatch('bottomchange', { atBottom: false });
      }
    }

    lastScrollTop = scrollTop;

    // Near the top — load previous chunk
    if (scrollTop < 80 && (visibleFrom > 0 || hasEarlierEntries)) {
      loadMore();
    }
  }

  /** Reveal a local chunk or request the previous persisted journal page.
   * @return Completion after the existing scroll anchor is restored.
   * @throws No propagated error; external page failures are handled by the parent panel.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-27
   */
  async function loadMore(): Promise<void> {
    if (visibleFrom === 0) {
      if (hasEarlierEntries && !loadingEarlierEntries && onLoadEarlier) {
        const previousScrollHeight = scrollerEl.scrollHeight;
        await onLoadEarlier();
        await tick();
        programmaticScroll = true;
        scrollerEl.scrollTop += scrollerEl.scrollHeight - previousScrollHeight;
        lastScrollTop = scrollerEl.scrollTop;
      }
      return;
    }
    // Capture anchor element before prepending items
    const anchor = scrollerEl.firstElementChild as HTMLElement | null;
    const anchorTop = anchor ? anchor.getBoundingClientRect().top : 0;

    const prevFrom = visibleFrom;
    visibleFrom = Math.max(0, visibleFrom - PAGE_SIZE);
    const added = prevFrom - visibleFrom; // how many items were actually prepended

    await tick();

    // Restore scroll so the old first item stays in the same visual position.
    // The "load more" button may occupy children[0], so account for it.
    const offset = visibleFrom > 0 ? 1 : 0; // 1 if "load more" button still present
    const anchorIdx = added + offset;
    const newAnchor = scrollerEl.children[anchorIdx] as HTMLElement | null;
    if (newAnchor) {
      programmaticScroll = true;
      scrollerEl.scrollTop += newAnchor.getBoundingClientRect().top - anchorTop;
      lastScrollTop = scrollerEl.scrollTop;
    }
  }

  // ── Auto-scroll to bottom ──────────────────────────────────────────────────
  export function scrollToBottom() {
    if (!scrollerEl) return;
    visibleFrom = Math.max(0, rows.length - PAGE_SIZE);
    isAtBottom = true;
    tick().then(() => {
      if (scrollerEl) {
        programmaticScroll = true;
        scrollerEl.scrollTop = scrollerEl.scrollHeight;
        lastScrollTop = scrollerEl.scrollTop;
      }
    });
  }

  // Auto-scroll when content grows (new entries OR existing entries expanding).
  // A ResizeObserver on the scroller catches every scrollHeight change — covers
  // streaming progress, markdown rendering, code blocks expanding, etc.
  let resizeObs: ResizeObserver | undefined;

  /** Observe only the timeline and viewport so content growth keeps the feed pinned efficiently.
   * @return Cleanup callback that disconnects the shared resize observer.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-27
   */
  function observeFeedSize(): () => void {
    feedMounted = true;
    knownEntryCount = entries.length;
    knownLastEntryIdentity = entries.length ? entryIdentity(entries[entries.length - 1]) : '';
    if (scrollerEl) scrollerEl.scrollTop = scrollerEl.scrollHeight;

    resizeObs = new ResizeObserver(() => {
      if (isAtBottom && scrollerEl) {
        programmaticScroll = true;
        scrollerEl.scrollTop = scrollerEl.scrollHeight;
        lastScrollTop = scrollerEl.scrollTop;
      }
    });

    resizeObs.observe(timelineEl);
    resizeObs.observe(scrollerEl);

    return () => resizeObs?.disconnect();
  }

  onMount(observeFeedSize);
  onDestroy(stopWorkingClock);
</script>

<div class="feed-scroller" class:compact bind:this={scrollerEl} onscroll={onScroll}>
  <div class="timeline" bind:this={timelineEl}>
    {#if hasMore}
      <button type="button" class="load-more" onclick={loadMore} disabled={loadingEarlierEntries}>
        {loadingEarlierEntries ? 'loading…' : 'load earlier'}
      </button>
    {/if}

    {#each visibleItems as row, i (visibleFrom + i)}
      {@const absIdx = visibleFrom + i}
      {#if row.kind === 'group'}
        {@const gOpen = expandedGroups.has(absIdx)}
        <article class="timeline-event tool" aria-label="{row.items.length} {row.tool} steps">
          <div class="timeline-node tool" aria-hidden="true"></div>
          <div class="timeline-body">
            <button
              class="tool-group-toggle"
              class:open={gOpen}
              onclick={() => toggleGroup(absIdx)}
              aria-expanded={gOpen}
            >
              <svg class="chev" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                <path
                  d="M6 4l4 4-4 4"
                  stroke="currentColor"
                  stroke-width="1.6"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                />
              </svg>
              {row.items.length}
              {row.tool}
              {row.items.length === 1 ? 'step' : 'steps'}
            </button>
            {#if gOpen}
              <div class="tool-group-items">
                {#each row.items as gi (gi.entry.seq)}
                  <ToolCallEntry
                    entry={gi.entry}
                    resultEntry={gi.result}
                    streamingEntries={gi.streaming}
                    {cwd}
                    {compact}
                  />
                {/each}
              </div>
            {/if}
          </div>
        </article>
      {:else}
        {@const item = row.item}
        {@const entry = item.entry}
        <article
          class="timeline-event {eventClass(entry)}"
          aria-label="{actorLabel(entry)} {ts(entry)}"
        >
          <div class="timeline-node {eventClass(entry)}" aria-hidden="true"></div>
          <div class="timeline-body">
            <div class="event-meta">
              <span class="event-actor {eventClass(entry)}">{actorLabel(entry)}</span>
              {#if ts(entry)}<span>{ts(entry)}</span>{/if}
            </div>

            {#if entry.entryType === 'toolCall'}
              <ToolCallEntry
                {entry}
                resultEntry={item.result}
                streamingEntries={item.streaming}
                {cwd}
                {compact}
              />
            {:else if entry.entryType === 'thinking'}
              {@const expanded = expandedThinking.has(absIdx)}
              <div class="thinking-inline" class:expanded>
                <span
                  class="think-dots-label"
                  onclick={() => toggleThinking(absIdx)}
                  role="button"
                  tabindex="0"
                  onkeydown={(e) => e.key === 'Enter' && toggleThinking(absIdx)}
                >
                  <span class="think-dots">
                    <span></span><span></span><span></span>
                  </span>
                  <span class="think-preview-text">
                    {#if expanded}
                      {(entry.thinking ?? '').split('\n')[0].slice(0, 60)}
                    {:else}
                      {(entry.thinking ?? '').split('\n')[0].slice(0, 80)}…
                    {/if}
                  </span>
                </span>
                <button
                  class="think-chip"
                  class:open={expanded}
                  onclick={() => toggleThinking(absIdx)}
                  aria-label={expanded ? 'collapse thinking' : 'expand thinking'}
                  aria-expanded={expanded}
                >
                  <svg class="chev" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                    <path
                      d="M6 4l4 4-4 4"
                      stroke="currentColor"
                      stroke-width="1.6"
                      stroke-linecap="round"
                      stroke-linejoin="round"
                    />
                  </svg>
                  {#if entry.thinkingDuration}{entry.thinkingDuration.toFixed(1)}s{/if}
                </button>
              </div>
              {#if expanded}
                <div class="event-text think-body">{entry.thinking}</div>
              {/if}
            {:else if entry.entryType === 'assistant'}
              <div class="event-text assistant-text">
                <Markdown
                  content={entry.text ?? ''}
                  stream={streamingAssistantIdentity === entryIdentity(entry)}
                />
              </div>
            {:else if entry.entryType === 'user'}
              <div class="event-text user-text">{entry.text}</div>
            {:else}
              <div class="event-text system-text" class:system-error={entry.feedError}>
                {entry.text}
              </div>
            {/if}
          </div>
        </article>
      {/if}
    {/each}

    {#if isWorking}
      <article class="timeline-event working" aria-label="agent working">
        <div class="timeline-node working" aria-hidden="true"></div>
        <div class="timeline-body">
          <div class="event-meta">
            <span class="event-actor working">Orbit-mode / {agentLabel}</span>
          </div>
          <div class="working-pill" role="status" aria-live="polite">
            <span class="working-word">working</span>
            <span class="working-elapsed" aria-hidden="true">· {workingElapsedSeconds}s</span>
            <span class="typing-dots" aria-hidden="true">
              <span class="dot"></span>
              <span class="dot"></span>
              <span class="dot"></span>
            </span>
          </div>
        </div>
      </article>
    {/if}
  </div>
</div>

<style>
  .feed-scroller {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .load-more {
    display: block;
    margin: 0 auto 4px;
    background: none;
    border: 1px solid var(--bd1);
    border-radius: var(--radius-sm);
    color: var(--t3);
    font-size: var(--xs);
    padding: var(--sp-2) var(--sp-5);
    cursor: pointer;
  }
  .load-more:hover {
    border-color: var(--ac);
    color: var(--ac);
  }

  .timeline {
    width: 100%;
    padding: 10px 18px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-family: var(--font-mono, var(--mono), monospace);
    font-variant-ligatures: none;
  }

  .timeline-event {
    display: grid;
    grid-template-columns: 12px 1fr;
    gap: 6px;
    position: relative;
    border-radius: 4px;
    transition: background 0.15s ease;
  }
  .timeline-event.user {
    background: linear-gradient(135deg, rgba(79, 146, 247, 0.08) 0%, rgba(79, 146, 247, 0.02) 100%);
    border: 1px solid rgba(79, 146, 247, 0.22);
    border-left: 3px solid var(--user-fg, #4f92f7);
    padding: 6px 9px;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.03);
  }
  .timeline-event:not(:last-child)::before {
    display: none;
  }

  .timeline-node {
    width: 12px;
    height: 16px;
    display: flex;
    align-items: center;
    justify-content: center;
    margin-top: 0;
    border: none;
    background: transparent;
  }
  .timeline-node::after {
    content: '›';
    font-family: var(--mono);
    font-size: 11px;
    font-weight: 700;
    color: var(--t3);
    background: transparent;
    box-shadow: none;
  }
  .timeline-node.user::after {
    content: '›';
    color: var(--user-fg, #4f92f7);
    font-size: 12px;
    font-weight: 700;
    text-shadow: 0 0 8px rgba(79, 146, 247, 0.5);
    background: transparent;
    box-shadow: none;
  }
  .timeline-node.assistant::after,
  .timeline-node.working::after {
    content: '•';
    color: var(--ac);
    font-size: 11px;
    background: transparent;
    box-shadow: none;
  }
  .timeline-node.tool::after {
    content: '›';
    color: var(--t3);
    font-size: 12px;
    background: transparent;
    box-shadow: none;
  }

  .event-meta {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 1px;
    color: var(--t3);
    font-family: var(--mono);
    font-size: 10px;
  }
  .event-actor {
    font-weight: 600;
    font-family: var(--mono);
    color: var(--t2);
  }
  .event-actor.user {
    color: var(--user-fg, #4f92f7);
    font-weight: 700;
    font-size: 9px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    background: rgba(79, 146, 247, 0.14);
    border: 1px solid rgba(79, 146, 247, 0.28);
    border-radius: 4px;
    padding: 0 4px;
  }
  .event-actor.assistant,
  .event-actor.working {
    color: var(--ac);
  }
  .event-ts {
    font-size: 10px;
    color: var(--t3);
    opacity: 0.6;
  }

  .event-text {
    color: var(--t0);
    font-size: 12px;
    line-height: 1.45;
  }
  .user-text {
    max-width: 100%;
    width: 100%;
    border: none;
    border-radius: 0;
    padding: 1px 0 0;
    background: transparent;
    font-family: var(--font-mono, var(--mono), monospace);
    font-size: 12px;
    line-height: 1.45;
    color: var(--t0, #ffffff);
    white-space: pre-wrap;
    word-break: break-word;
    font-weight: 500;
  }
  .system-text {
    color: var(--t1);
    font-family: var(--mono);
    font-size: 11px;
  }
  .system-text.system-error {
    color: var(--t1);
    border-left: 2px solid var(--s-error);
    padding: 2px 0 2px 8px;
    background: none;
    border-radius: 0;
  }

  .working-pill {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--think-fg, #a78bfa);
    background: var(--think-bg, rgba(167, 139, 250, 0.08));
    border: 1px solid color-mix(in srgb, var(--think-fg, #a78bfa), transparent 82%);
    border-radius: 999px;
    padding: 3px 7px;
    font-family: var(--mono);
    font-size: 9.5px;
    line-height: 1;
  }

  .working-word {
    letter-spacing: 0.02em;
  }

  .working-elapsed {
    color: var(--t3);
    font-variant-numeric: tabular-nums;
  }

  .typing-dots {
    display: inline-flex;
    align-items: flex-end;
    gap: 3px;
    height: 8px;
    padding-bottom: 1px;
  }

  .typing-dots .dot {
    width: 3.5px;
    height: 3.5px;
    border-radius: 50%;
    background: currentColor;
    opacity: 0.35;
    animation: typingDot 1.2s ease-in-out infinite;
  }

  .typing-dots .dot:nth-child(2) {
    animation-delay: 0.15s;
  }

  .typing-dots .dot:nth-child(3) {
    animation-delay: 0.3s;
  }

  @keyframes typingDot {
    0%,
    70%,
    100% {
      opacity: 0.35;
      transform: translateY(0);
    }
    35% {
      opacity: 1;
      transform: translateY(-2.5px);
    }
  }

  /* ── Thinking inline ── */
  .thinking-inline {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .think-dots-label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    color: var(--tool-fg);
    flex: 1;
    min-width: 0;
  }
  .think-dots {
    display: inline-flex;
    gap: 3px;
    align-items: center;
  }
  .think-dots span {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--tool-fg);
    opacity: 0.4;
    animation: thinkDot 1.4s ease-in-out infinite;
  }
  .think-dots span:nth-child(2) {
    animation-delay: 0.2s;
  }
  .think-dots span:nth-child(3) {
    animation-delay: 0.4s;
  }
  @keyframes thinkDot {
    0%,
    80%,
    100% {
      opacity: 0.4;
      transform: translateY(0);
    }
    40% {
      opacity: 1;
      transform: translateY(-3px);
    }
  }
  .think-preview-text {
    font-size: var(--sm);
    color: var(--tool-fg);
    opacity: 0.6;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: var(--mono);
  }
  .thinking-inline.expanded .think-preview-text {
    opacity: 1;
    font-family: var(--mono);
    color: var(--think-fg);
  }
  .think-body {
    color: var(--think-fg);
    white-space: pre-wrap;
    font-style: italic;
    font-size: var(--sm);
    background: var(--think-bg);
    border-left: 2px solid var(--think-fg);
    padding: 6px 9px;
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
    max-height: 280px;
    overflow-y: auto;
    margin-top: 3px;
  }

  .tool-group-toggle {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    background: var(--bg2);
    border: 1px solid var(--bd1);
    border-radius: 999px;
    color: var(--t1);
    font-family: var(--mono);
    font-size: 10px;
    padding: 3px 8px;
    cursor: pointer;
    transition:
      color 0.15s,
      border-color 0.15s;
  }
  .tool-group-toggle:hover {
    color: var(--tool-fg);
    border-color: color-mix(in srgb, var(--tool-fg), transparent 60%);
  }
  .tool-group-toggle .chev {
    width: 9px;
    height: 9px;
    transition: transform 0.18s ease;
  }
  .tool-group-toggle.open .chev {
    transform: rotate(90deg);
  }
  .tool-group-items {
    margin-top: 5px;
    display: flex;
    flex-direction: column;
  }

  .think-chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    flex-shrink: 0;
    background: none;
    border: 1px solid var(--bd1);
    border-radius: 999px;
    color: var(--t2);
    font-size: 10px;
    font-family: var(--mono);
    line-height: 1;
    padding: 3px 9px 3px 7px;
    cursor: pointer;
    transition:
      color 0.15s,
      border-color 0.15s;
  }
  .think-chip:hover {
    color: var(--think-fg);
    border-color: color-mix(in srgb, var(--think-fg), transparent 60%);
  }
  .think-chip .chev {
    width: 9px;
    height: 9px;
    transition: transform 0.18s ease;
  }
  .think-chip.open .chev {
    transform: rotate(90deg);
  }

  /* ── Compact density ── */
  .feed-scroller.compact .timeline {
    width: 100%;
    padding: 8px 16px;
    gap: 6px;
  }
  .feed-scroller.compact .timeline-event {
    grid-template-columns: 10px 1fr;
    gap: 5px;
  }
  .feed-scroller.compact .timeline-node {
    width: 10px;
    height: 14px;
  }
  .feed-scroller.compact .timeline-node::after {
    width: 6px;
    height: 6px;
  }
  .feed-scroller.compact .timeline-event:not(:last-child)::before {
    left: 7px;
    top: 24px;
    bottom: -10px;
  }
  .feed-scroller.compact .event-text {
    font-size: 11px;
    line-height: 1.4;
  }
  .feed-scroller.compact .user-text {
    border: 0;
    background: transparent;
    padding: 0;
    max-width: none;
  }

  @media (max-width: 768px) {
    .timeline {
      padding: 10px 12px;
      gap: 7px;
    }
    .think-body {
      max-height: 180px;
    }
  }
</style>
