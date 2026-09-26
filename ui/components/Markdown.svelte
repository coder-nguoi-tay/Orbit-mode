<script lang="ts">
  import { onDestroy } from 'svelte';
  import { marked } from 'marked';
  import { highlightCode } from '../lib/highlight';
  import { prefersReducedMotion } from '../lib/motion';

  export let content: string;
  export let stream = false;

  let displayedContent = stream ? '' : content;
  let streamedContent = '';
  let streamFrame: number | null = null;

  /** Stop the active assistant-text reveal animation.
   * @return No value; any pending animation frame is cancelled.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-27
   */
  function stopStreaming(): void {
    if (streamFrame !== null) cancelAnimationFrame(streamFrame);
    streamFrame = null;
  }

  /** Reveal a newly received assistant response in short readable chunks.
   * @param nextContent Complete assistant text received from the provider CLI.
   * @return No value; visible text advances over at most about 45 frames.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-27
   */
  function streamContent(nextContent: string): void {
    stopStreaming();
    streamedContent = nextContent;

    if (!stream || prefersReducedMotion() || !nextContent) {
      displayedContent = nextContent;
      return;
    }

    let visibleLength = nextContent.startsWith(displayedContent) ? displayedContent.length : 0;
    const chunkSize = Math.max(8, Math.ceil(nextContent.length / 45));
    displayedContent = nextContent.slice(0, visibleLength);

    /** Advance the current response to the next nearby word boundary.
     * @return No value; schedules another frame until the response is complete.
     * @author ductv <ductv@getflycrm.com>
     * @since 2026-09-27
     */
    const revealNextChunk = (): void => {
      const targetLength = Math.min(nextContent.length, visibleLength + chunkSize);
      const nearbyBreak = nextContent.slice(targetLength, targetLength + 24).search(/[\s.,!?;:]/);
      visibleLength = Math.min(
        nextContent.length,
        nearbyBreak >= 0 ? targetLength + nearbyBreak + 1 : targetLength
      );
      displayedContent = nextContent.slice(0, visibleLength);
      if (visibleLength < nextContent.length) {
        streamFrame = requestAnimationFrame(revealNextChunk);
      } else {
        streamFrame = null;
      }
    };

    streamFrame = requestAnimationFrame(revealNextChunk);
  }

  marked.setOptions({
    breaks: false,
    gfm: true,
  });

  marked.use({
    renderer: {
      code({ text, lang }) {
        const language = lang?.trim() || undefined;
        const highlighted = highlightCode(text, language);
        const cls = language ? `hljs language-${language}` : 'hljs';
        return `<pre><code class="${cls}">${highlighted}</code></pre>\n`;
      },
    },
  });

  $: if (!stream) {
    stopStreaming();
    streamedContent = content;
    displayedContent = content;
  } else if (content !== streamedContent) {
    streamContent(content);
  }

  $: html = marked.parse(displayedContent || '') as string;

  onDestroy(stopStreaming);
</script>

<div class="md" class:streaming={stream && displayedContent.length < content.length}>
  {@html html}
  {#if stream && displayedContent.length < content.length}<span
      class="stream-caret"
      aria-hidden="true">▋</span
    >{/if}
</div>

<style>
  .md :global(p) {
    margin: 0 0 4px 0;
  }
  .md :global(p:last-child) {
    margin-bottom: 0;
  }
  .md :global(p:only-child) {
    margin: 0;
  }
  .md :global(strong) {
    font-weight: 700;
    color: var(--t0);
  }
  .md :global(em) {
    font-style: italic;
  }
  .md :global(code) {
    font-family: 'Cascadia Code', 'Fira Code', monospace;
    background: var(--bg3);
    padding: 0 4px;
    border-radius: var(--radius-sm);
    font-size: 0.9em;
  }
  .md :global(pre) {
    background: var(--bg3);
    border: 1px solid var(--bd1);
    padding: 7px 9px;
    border-radius: var(--radius-sm);
    overflow-x: auto;
    margin: 5px 0;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .md :global(pre code) {
    background: none;
    padding: 0;
  }
  .md :global(ul),
  .md :global(ol) {
    margin: 4px 0;
    padding-left: 1.8em;
  }
  .md :global(li) {
    margin: 2px 0;
    padding-left: 2px;
  }
  .md :global(h1),
  .md :global(h2),
  .md :global(h3) {
    font-weight: 700;
    margin: 7px 0 3px;
    color: var(--t0);
  }
  .md :global(h1) {
    font-size: 1.2em;
  }
  .md :global(h2) {
    font-size: 1.1em;
  }
  .md :global(h3) {
    font-size: 1em;
  }
  .md :global(blockquote) {
    border-left: 3px solid var(--bd2);
    padding-left: 8px;
    margin: 4px 0;
    color: var(--t1);
  }
  .md :global(a) {
    color: var(--ac);
    text-decoration: none;
  }
  .md :global(a:hover) {
    text-decoration: underline;
  }
  .md :global(hr) {
    border: none;
    border-top: 1px solid var(--bd1);
    margin: 7px 0;
  }
  .md :global(table) {
    border-collapse: collapse;
    width: 100%;
    margin: 5px 0;
    font-size: 0.9em;
  }
  .md :global(th),
  .md :global(td) {
    border: 1px solid var(--bd2);
    padding: 4px 7px;
    text-align: left;
  }
  .md :global(th) {
    background: var(--bg3);
    font-weight: 600;
    color: var(--t0);
  }
  .md :global(tr:nth-child(even) td) {
    background: var(--bg2);
  }

  .stream-caret {
    display: inline-block;
    margin-left: 2px;
    color: var(--ac);
    animation: streamCaret 0.7s steps(1) infinite;
  }

  @keyframes streamCaret {
    50% {
      opacity: 0.15;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .stream-caret {
      animation: none;
    }
  }
</style>
