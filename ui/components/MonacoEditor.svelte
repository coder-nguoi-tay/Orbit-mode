<script lang="ts">
  import { createEventDispatcher, onDestroy, onMount } from 'svelte';
  import * as monaco from 'monaco-editor';
  import EditorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker';
  import TsWorker from 'monaco-editor/esm/vs/language/typescript/ts.worker?worker';
  import JsonWorker from 'monaco-editor/esm/vs/language/json/json.worker?worker';
  import HtmlWorker from 'monaco-editor/esm/vs/language/html/html.worker?worker';
  import CssWorker from 'monaco-editor/esm/vs/language/css/css.worker?worker';

  export let content: string = '';
  export let language = 'plaintext';
  export let readOnly = false;
  export const filePath: string | null = null;

  const dispatch = createEventDispatcher<{
    save: { content: string };
    dirty: { dirty: boolean };
  }>();

  let host: HTMLDivElement;
  let editor: monaco.editor.IStandaloneCodeEditor | null = null;
  let editorReady = false;
  let _dirty = false;
  let _contentListener: monaco.IDisposable | null = null;
  let _initFrame: ReturnType<typeof requestAnimationFrame> | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let _prevContent = '';
  let _prevLanguage = '';

  /** Configure Monaco workers for the language services used by the file editor.
   * @return No value; installs the worker resolver on the current window.
   * @author ductv <ductv@getflycrm.com>
   * @since 2026-09-26
   */
  function ensureWorkers() {
    const g = self as unknown as {
      MonacoEnvironment?: { getWorker(_id: string, _label: string): Worker };
    };
    g.MonacoEnvironment = {
      getWorker: (_id: string, label: string) => {
        if (label === 'typescript' || label === 'javascript') return new TsWorker();
        if (label === 'json') return new JsonWorker();
        if (label === 'html' || label === 'handlebars' || label === 'razor')
          return new HtmlWorker();
        if (label === 'css' || label === 'scss' || label === 'less') return new CssWorker();
        return new EditorWorker();
      },
    };
  }

  function applyOrbittTheme() {
    monaco.editor.defineTheme('orbit-dark', {
      base: 'vs-dark',
      inherit: true,
      rules: [
        { token: 'comment', foreground: '67615a', fontStyle: 'italic' },
        { token: 'keyword', foreground: 'c8a8ff', fontStyle: 'bold' },
        { token: 'string', foreground: '7cff9e' },
        { token: 'number', foreground: 'f2c96f' },
        { token: 'type', foreground: 'e4b77d' },
        { token: 'function', foreground: '4da3ff' },
        { token: 'variable', foreground: 'f1ece3' },
        { token: 'constant', foreground: 'ff7b72' },
        { token: 'delimiter', foreground: 'a19a90' },
      ],
      colors: {
        'editor.background': '#090a0a',
        'editor.foreground': '#f1ece3',
        'editorLineNumber.foreground': '#45413c',
        'editorLineNumber.activeForeground': '#c4bdb3',
        'editorGutter.background': '#090a0a',
        'editor.selectionBackground': '#7cff9e26',
        'editor.inactiveSelectionBackground': '#7cff9e12',
        'editor.selectionHighlightBackground': '#7cff9e18',
        'editor.lineHighlightBackground': '#ffffff06',
        'editor.lineHighlightBorder': '#00000000',
        'editorCursor.foreground': '#7cff9e',
        'editorWhitespace.foreground': '#ffffff0d',
        'editorIndentGuide.background1': '#ffffff0a',
        'editorIndentGuide.activeBackground1': '#ffffff18',
        'scrollbarSlider.background': '#ffffff12',
        'scrollbarSlider.hoverBackground': '#ffffff22',
        'scrollbarSlider.activeBackground': '#ffffff32',
      },
    });
    monaco.editor.setTheme('orbit-dark');
  }

  function bindContent() {
    _contentListener?.dispose();
    _contentListener = null;
    if (!editor) return;
    const model = editor.getModel();
    if (!model) return;

    _prevContent = content;
    _dirty = false;
    dispatch('dirty', { dirty: false });

    _contentListener = model.onDidChangeContent(() => {
      const val = editor?.getModel()?.getValue() ?? '';
      const nowDirty = val !== content;
      if (nowDirty !== _dirty) {
        _dirty = nowDirty;
        dispatch('dirty', { dirty: nowDirty });
      }
    });
  }

  export function markSaved() {
    if (!editor) return;
    const val = editor.getModel()?.getValue() ?? '';
    _prevContent = val;
    _dirty = false;
    dispatch('dirty', { dirty: false });
  }

  export function getValue(): string {
    return editor?.getModel()?.getValue() ?? '';
  }

  onMount(() => {
    ensureWorkers();
    _initFrame = requestAnimationFrame(() => {
      _initFrame = null;
      if (!host) return;
      applyOrbittTheme();
      editor = monaco.editor.create(host, {
        value: content,
        language,
        theme: 'orbit-dark',
        readOnly,
        automaticLayout: true,
        minimap: { enabled: false },
        folding: true,
        foldingStrategy: 'indentation',
        scrollBeyondLastLine: false,
        fontSize: 13,
        lineHeight: 21,
        fontFamily: "'JetBrains Mono', 'Cascadia Code', 'Fira Code', Menlo, Consolas, monospace",
        fontLigatures: true,
        renderLineHighlight: 'all',
        cursorBlinking: 'smooth',
        cursorSmoothCaretAnimation: 'on',
        smoothScrolling: true,
        lineNumbersMinChars: 3,
        roundedSelection: true,
        scrollbar: {
          vertical: 'visible',
          horizontal: 'visible',
          verticalScrollbarSize: 9,
          horizontalScrollbarSize: 9,
          useShadows: false,
        },
        padding: { top: 10, bottom: 10 },
      });

      editor.addAction({
        id: 'orbit-save',
        label: 'Save File',
        keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS],
        run: () => {
          const val = editor?.getModel()?.getValue() ?? '';
          dispatch('save', { content: val });
        },
      });

      bindContent();
      editorReady = true;

      // Ensure full-width sizing via ResizeObserver
      if (typeof ResizeObserver !== 'undefined' && host) {
        resizeObserver = new ResizeObserver(() => {
          if (editor) {
            editor.layout();
          }
        });
        resizeObserver.observe(host);
      }

      // Initial layout pass
      setTimeout(() => {
        editor?.layout();
      }, 50);
    });
  });

  // Re-set content when it changes externally (file switch)
  $: if (editor && editorReady && (content !== _prevContent || language !== _prevLanguage)) {
    const model = editor.getModel();
    if (model) {
      _prevContent = content;
      _prevLanguage = language;
      model.setValue(content);
      monaco.editor.setModelLanguage(model, language);
    }
    bindContent();
  }

  $: if (editor && editorReady) {
    editor.updateOptions({ readOnly });
  }

  onDestroy(() => {
    if (_initFrame !== null) cancelAnimationFrame(_initFrame);
    resizeObserver?.disconnect();
    resizeObserver = null;
    _contentListener?.dispose();
    editor?.dispose();
    editor = null;
  });
</script>

<div class="editor-shell">
  <div class="editor-host" bind:this={host}></div>
  {#if !editorReady}
    <div class="editor-loading">
      <div class="spinner"></div>
      <span>Loading editor…</span>
    </div>
  {/if}
</div>

<style>
  .editor-shell {
    position: relative;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex: 1;
    flex-direction: column;
  }

  .editor-host {
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    flex: 1;
  }

  .editor-loading {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--t2);
    background: var(--bg);
    font-size: 11px;
    font-family: var(--mono);
  }

  .spinner {
    width: 16px;
    height: 16px;
    border: 2px solid var(--bd1);
    border-top-color: var(--ac);
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
