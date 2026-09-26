<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { ptyCreate, ptyWrite, ptyResize, ptyKill, onPtyOutput } from '../lib/tauri/terminal';
  import PanelHeader from './workspace/PanelHeader.svelte';
  import { shortenPath } from '../lib/path';
  import { Trash2, RotateCw, Terminal as TerminalIcon } from 'lucide-svelte';
  import '@xterm/xterm/css/xterm.css';

  let {
    sessionId = 0,
    terminalId = '',
    cwd = '.',
    focused = true,
    onClose = null,
  }: {
    sessionId?: number;
    terminalId?: string;
    cwd?: string;
    focused?: boolean;
    onClose?: (() => void) | null;
  } = $props();

  let container: HTMLDivElement | undefined = $state();
  let terminal: Terminal | undefined = $state();
  let fitAddon: FitAddon | undefined = $state();
  let unlisten: (() => void) | undefined = $state();
  let resizeObserver: ResizeObserver | undefined = $state();

  let loading = $state(false);
  let error = $state('');
  // The numeric PTY id used for all pty* calls.
  let numericId = $state(0);
  // Whether we spawned the PTY ourselves (and must kill it on destroy).
  let ownedPty = $state(false);

  /** Derive a stable numeric id from a string by summing char codes. */
  function hashString(s: string): number {
    let h = 0;
    for (let i = 0; i < s.length; i++) {
      h = (Math.imul(31, h) + s.charCodeAt(i)) | 0;
    }
    // Ensure positive, non-zero
    return Math.abs(h) || Date.now();
  }

  function resolveNumericId(): number {
    if (sessionId > 0) return sessionId;
    if (terminalId) return hashString(terminalId);
    return Date.now();
  }

  function cyberpunkTermTheme(): Terminal['options']['theme'] {
    return {
      background: '#060709',
      foreground: '#e2fbe8',
      cursor: '#00ff88',
      cursorAccent: '#060709',
      selectionBackground: 'rgba(0, 255, 136, 0.26)',
      selectionForeground: '#ffffff',
      black: '#121517',
      red: '#ff5555',
      green: '#00ff88',
      yellow: '#f1fa8c',
      blue: '#57c7ff',
      magenta: '#ff79c6',
      cyan: '#00e5ff',
      white: '#e2fbe8',
      brightBlack: '#434d52',
      brightRed: '#ff6e6e',
      brightGreen: '#50fa7b',
      brightYellow: '#ffffa5',
      brightBlue: '#8be9fd',
      brightMagenta: '#ff92d0',
      brightCyan: '#8be9fd',
      brightWhite: '#ffffff',
    };
  }

  async function spawnPty(term: Terminal, fit: FitAddon): Promise<void> {
    const isWindows = typeof navigator !== 'undefined' && navigator.platform.startsWith('Win');
    const isMac = typeof navigator !== 'undefined' && (navigator.platform.startsWith('Mac') || navigator.userAgent.includes('Mac'));
    const shell = isWindows ? 'powershell.exe' : (isMac ? '/bin/zsh' : '/bin/bash');
    const args = isWindows ? [] : ['-l'];
    
    // Get current terminal dimensions before spawning
    fit.fit();
    const rows = term.rows || 24;
    const cols = term.cols || 80;

    await ptyCreate(numericId, shell, args, cwd, [], rows, cols);
  }

  async function initTerminal(): Promise<void> {
    if (!container) return;

    loading = true;
    error = '';

    const term = new Terminal({
      cursorBlink: true,
      cursorStyle: 'block',
      fontSize: 13,
      lineHeight: 1.25,
      fontFamily: "'JetBrains Mono', 'Fira Code', Menlo, Monaco, Consolas, monospace",
      scrollback: 5000,
      theme: cyberpunkTermTheme(),
      allowTransparency: true,
      fontWeight: '400',
      fontWeightBold: '700',
    });

    const fit = new FitAddon();
    term.loadAddon(fit);

    term.open(container);

    // Hide native scrollbar (Windows WebView2 ignores ::-webkit-scrollbar)
    const vp = container.querySelector('.xterm-viewport') as HTMLElement | null;
    if (vp) {
      vp.style.setProperty('right', '-23px', 'important');
      vp.style.setProperty('overflow-y', 'auto', 'important');
    }
    if (container) {
      container.style.setProperty('overflow', 'hidden', 'important');
    }

    numericId = resolveNumericId();

    // 1. Hook up data and resize handlers
    term.onData(async (data) => {
      try {
        await ptyWrite(numericId, data);
      } catch (e) {
        console.error('pty write error:', e);
      }
    });

    term.onResize(async ({ cols, rows }) => {
      try {
        await ptyResize(numericId, rows, cols);
      } catch (e) {
        console.error('pty resize error:', e);
      }
    });

    // 2. Hook up output listener BEFORE spawning to ensure initial output is never lost
    unlisten = await onPtyOutput(({ sessionId: sid, data, eof }) => {
      if (sid !== numericId) return;
      if (eof) {
        term?.writeln('\r\n\x1b[38;2;0;255;136m[process exited]\x1b[0m');
        return;
      }
      term?.write(data);
    });

    // 3. Auto-spawn a shell when no external sessionId drives the PTY
    if (sessionId <= 0) {
      try {
        await spawnPty(term, fit);
        ownedPty = true;
      } catch (e) {
        error = e instanceof Error ? e.message : String(e);
        loading = false;
        term.dispose();
        return;
      }
    }

    loading = false;
    terminal = term;
    fitAddon = fit;

    // Initial fit & focus
    requestAnimationFrame(() => {
      fit.fit();
      term.focus();
    });

    let resizeTimer: ReturnType<typeof setTimeout>;
    resizeObserver = new ResizeObserver(() => {
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => {
        fit?.fit();
      }, 40);
    });
    resizeObserver.observe(container);
  }

  function clearTerminal() {
    terminal?.clear();
  }

  function restartTerminal() {
    terminal?.dispose();
    terminal = undefined;
    if (ownedPty) {
      ptyKill(numericId).catch(() => {});
    }
    initTerminal();
  }

  onMount(async () => {
    await initTerminal();
  });

  onDestroy(() => {
    resizeObserver?.disconnect();
    unlisten?.();
    terminal?.dispose();
    if (ownedPty) {
      ptyKill(numericId).catch((e) => console.error('pty kill error:', e));
    }
  });
</script>

<section class="terminal-shell">
  <div class="terminal-header-bar">
    <PanelHeader
      title="Terminal"
      path={cwd ? shortenPath(cwd) : null}
      pathFull={cwd}
      {onClose}
      {focused}
    />
    <div class="terminal-quick-tools">
      <span class="cyber-badge">
        <span class="pulse-dot"></span>
        <span>ZSH</span>
      </span>
      <button class="tool-btn" onclick={clearTerminal} title="Clear Terminal (Cmd+K)" type="button">
        <Trash2 size={11} />
        <span>Clear</span>
      </button>
      <button class="tool-btn" onclick={restartTerminal} title="Restart Shell" type="button">
        <RotateCw size={11} />
      </button>
    </div>
  </div>

  <div class="terminal-body">
    {#if loading}
      <div class="terminal-overlay">
        <div class="cyber-spinner"></div>
        <span class="terminal-status">INITIALIZING SHELL...</span>
      </div>
    {:else if error}
      <div class="terminal-overlay">
        <span class="terminal-status error">{error}</span>
        <button
          class="retry-btn"
          onclick={restartTerminal}>Retry</button
        >
      </div>
    {/if}

    <div class="terminal-panel" bind:this={container} class:hidden={!!error || loading}></div>
  </div>
</section>

<style>
  .terminal-shell {
    display: flex;
    flex-direction: column;
    flex: 1;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    background: #060709;
    color: #e2fbe8;
    margin: 0;
    border: none;
    outline: none;
    overflow: hidden;
    position: relative;
  }

  .terminal-header-bar {
    position: relative;
    display: flex;
    align-items: center;
    background: #060709;
    border-bottom: 1px solid rgba(0, 255, 136, 0.12);
  }

  .terminal-header-bar :global(header) {
    flex: 1;
    background: transparent !important;
    border: none !important;
  }

  .terminal-quick-tools {
    position: absolute;
    right: 36px;
    top: 50%;
    transform: translateY(-50%);
    display: flex;
    align-items: center;
    gap: 6px;
    z-index: 5;
  }

  .cyber-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 7px;
    border-radius: 4px;
    background: rgba(0, 255, 136, 0.08);
    border: 1px solid rgba(0, 255, 136, 0.25);
    color: #00ff88;
    font-size: 9px;
    font-family: var(--mono);
    font-weight: 700;
    letter-spacing: 0.06em;
  }

  .pulse-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: #00ff88;
    box-shadow: 0 0 6px #00ff88;
    animation: cyberPulse 1.8s infinite ease-in-out;
  }

  @keyframes cyberPulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.4; transform: scale(0.8); }
  }

  .tool-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 7px;
    border-radius: 4px;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    color: var(--t2);
    font-size: 10px;
    font-family: var(--mono);
    cursor: pointer;
    transition: all 0.12s ease;
  }

  .tool-btn:hover {
    background: rgba(0, 255, 136, 0.12);
    border-color: rgba(0, 255, 136, 0.3);
    color: #00ff88;
  }

  .terminal-body {
    flex: 1;
    min-height: 0;
    padding: 10px 14px;
    background: #060709;
    display: flex;
    width: 100%;
    height: 100%;
    overflow: hidden;
    margin: 0;
    position: relative;
    box-shadow: inset 0 1px 0 0 rgba(0, 255, 136, 0.12), inset 0 0 30px -10px rgba(0, 255, 136, 0.03);
  }

  .terminal-shell :global(.xterm) {
    font-family: 'JetBrains Mono', 'Fira Code', Menlo, Monaco, Consolas, monospace !important;
    -webkit-font-smoothing: antialiased;
  }

  .terminal-shell :global(.xterm-screen) {
    padding: 2px 0;
  }

  .terminal-panel {
    flex: 1;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    background: transparent;
    margin: 0;
    border: none;
    outline: none;
    overflow: hidden;
  }

  .terminal-overlay {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    flex: 1;
    padding: 24px;
  }

  .terminal-status {
    font-family: var(--mono);
    font-size: 11px;
    color: #00ff88;
    letter-spacing: 0.08em;
    opacity: 0.9;
  }

  .terminal-status.error {
    color: #ff5555;
    opacity: 1;
  }

  .cyber-spinner {
    width: 20px;
    height: 20px;
    border: 2px solid rgba(0, 255, 136, 0.15);
    border-top-color: #00ff88;
    border-radius: 50%;
    animation: spin 0.6s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .retry-btn {
    padding: 4px 14px;
    background: rgba(255, 85, 85, 0.15);
    border: 1px solid rgba(255, 85, 85, 0.35);
    border-radius: 4px;
    color: #ff8888;
    font-size: 11px;
    font-family: var(--mono);
    cursor: pointer;
    transition: all 0.15s;
  }

  .retry-btn:hover {
    background: rgba(255, 85, 85, 0.25);
  }
</style>
