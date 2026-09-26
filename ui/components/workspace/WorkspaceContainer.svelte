<script lang="ts">
  import { workspace } from '../../lib/stores/workspace';
  import type { SplitNode } from '../../lib/stores/workspace';
  import { resizeSplit } from '../../lib/stores/workspace';
  import SplitContainer from './SplitContainer.svelte';
  import PaneContainer from './PaneContainer.svelte';

  type LeafNode = Extract<SplitNode, { type: 'leaf' }>;
  type SimpleRootSplit = {
    type: 'split';
    direction: 'horizontal' | 'vertical';
    ratio: number;
    children: [LeafNode, LeafNode];
  };

  $: node = $workspace.root;
  $: simpleRootSplit =
    node.type === 'split' && node.children[0].type === 'leaf' && node.children[1].type === 'leaf'
      ? (node as SimpleRootSplit)
      : null;

  let retainedPaneId: string | null = null;
  let rootContainer: HTMLDivElement;
  let resizingRoot = false;

  $: {
    if (node.type === 'leaf') {
      retainedPaneId = node.paneId;
    } else if (simpleRootSplit) {
      const rootPaneIds = simpleRootSplit.children.map((child) => child.paneId);
      if (!retainedPaneId || !rootPaneIds.includes(retainedPaneId)) {
        retainedPaneId = simpleRootSplit.children[0].paneId;
      }
    } else {
      retainedPaneId = null;
    }
  }

  $: retainedPaneIndex = simpleRootSplit
    ? simpleRootSplit.children.findIndex((child) => child.paneId === retainedPaneId)
    : 0;
  $: siblingPaneId = simpleRootSplit
    ? simpleRootSplit.children[retainedPaneIndex === 0 ? 1 : 0].paneId
    : null;
  $: retainedRatio = simpleRootSplit
    ? retainedPaneIndex === 0
      ? simpleRootSplit.ratio
      : 1 - simpleRootSplit.ratio
    : 1;
  $: siblingRatio = 1 - retainedRatio;
  $: isHorizontal = simpleRootSplit?.direction !== 'vertical';
  $: retainedStyle = simpleRootSplit
    ? `${isHorizontal ? 'width' : 'height'}:${retainedRatio * 100}%;order:${retainedPaneIndex * 2}`
    : 'flex:1;order:0';
  $: siblingStyle = simpleRootSplit
    ? `${isHorizontal ? 'width' : 'height'}:${siblingRatio * 100}%;order:${retainedPaneIndex === 0 ? 2 : 0}`
    : '';

  /** Begin resizing the optimized two-pane root without replacing either pane instance.
   * @param event Pointer event that starts on the root resize handle.
   * @return No value.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-26
   */
  function handleRootResizeStart(event: PointerEvent): void {
    event.preventDefault();
    resizingRoot = true;
    window.addEventListener('pointermove', handleRootResizeMove);
    window.addEventListener('pointerup', handleRootResizeEnd, { once: true });
  }

  /** Resize the current root split from the active pointer position.
   * @param event Pointer event containing the current cursor position.
   * @return No value.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-26
   */
  function handleRootResizeMove(event: PointerEvent): void {
    if (!resizingRoot || !simpleRootSplit || !rootContainer) return;
    const bounds = rootContainer.getBoundingClientRect();
    const ratio = isHorizontal
      ? (event.clientX - bounds.left) / bounds.width
      : (event.clientY - bounds.top) / bounds.height;
    resizeSplit([], ratio);
  }

  /** Finish root resizing and remove the temporary global pointer listener.
   * @return No value.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-26
   */
  function handleRootResizeEnd(): void {
    resizingRoot = false;
    window.removeEventListener('pointermove', handleRootResizeMove);
  }

  /** Restore an even root split when the divider is double-clicked.
   * @return No value.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-26
   */
  function resetRootSplit(): void {
    resizeSplit([], 0.5);
  }
</script>

<div
  class="workspace"
  class:horizontal={isHorizontal}
  class:vertical={!isHorizontal}
  bind:this={rootContainer}
>
  {#if retainedPaneId}
    <div class="root-pane" style={retainedStyle}>
      <PaneContainer paneId={retainedPaneId} />
    </div>
  {/if}

  {#if simpleRootSplit && siblingPaneId}
    <div
      class="root-resize-handle"
      class:handle-horizontal={isHorizontal}
      class:handle-vertical={!isHorizontal}
      role="separator"
      aria-orientation={isHorizontal ? 'vertical' : 'horizontal'}
      aria-valuenow={Math.round(simpleRootSplit.ratio * 100)}
      aria-valuemin={15}
      aria-valuemax={85}
      tabindex="-1"
      on:pointerdown={handleRootResizeStart}
      on:dblclick={resetRootSplit}
    ></div>
    <div class="root-pane" style={siblingStyle}>
      <PaneContainer paneId={siblingPaneId} />
    </div>
  {:else if node.type === 'split'}
    <SplitContainer {node} path={[]} />
  {/if}
</div>

<style>
  .workspace {
    flex: 1;
    display: flex;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }

  .workspace.horizontal {
    flex-direction: row;
  }

  .workspace.vertical {
    flex-direction: column;
  }

  .root-pane {
    display: flex;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    flex-shrink: 0;
  }

  .root-resize-handle {
    order: 1;
    flex-shrink: 0;
    position: relative;
    z-index: 2;
    background: transparent;
  }

  .root-resize-handle::after {
    content: '';
    position: absolute;
    background: rgba(255, 255, 255, 0.08);
    border-radius: 999px;
    transition: background 0.15s;
  }

  .root-resize-handle:hover::after {
    background: color-mix(in srgb, var(--ac), transparent 65%);
  }

  .handle-horizontal {
    width: 2px;
    cursor: col-resize;
  }

  .handle-horizontal::after {
    width: 1px;
    height: 100%;
    inset: 0 -3px;
    margin: auto;
  }

  .handle-vertical {
    height: 2px;
    cursor: row-resize;
  }

  .handle-vertical::after {
    width: 100%;
    height: 1px;
    inset: -3px 0;
    margin: auto;
  }
</style>
