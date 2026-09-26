<script lang="ts">
  import {
    FileCode,
    FileText,
    Folder,
    FolderOpen,
    Code2,
    Save,
    Search,
    X,
    Check,
    AlertCircle,
    ChevronRight,
  } from 'lucide-svelte';
  import { readFileContent } from '../lib/tauri/files';
  import { writeFileContent } from '../lib/tauri/git';
  import { listProjectFiles } from '../lib/tauri/projects';
  import PanelHeader from './workspace/PanelHeader.svelte';
  import MonacoEditor from './MonacoEditor.svelte';
  import { shortenPath } from '../lib/path';
  import { addToast } from '../lib/stores/toasts';
  import { onMount } from 'svelte';

  function toast(message: string, type: 'success' | 'error') {
    addToast({ type, message, autoDismiss: true });
  }

  export let cwd: string;
  export let onClose: (() => void) | null = null;
  export let focused: boolean = true;

  const MAX_BINARY_SIZE = 500 * 1024;
  const BINARY_EXTENSIONS = new Set([
    // Images
    'png', 'jpg', 'jpeg', 'gif', 'bmp', 'ico', 'webp', 'tiff', 'tif', 'psd', 'ai', 'raw', 'heic', 'heif', 'avif', 'eps', 'indd', 'svgz',
    // Audio
    'mp3', 'wav', 'ogg', 'm4a', 'aac', 'flac', 'wma', 'aiff', 'opus', 'mid', 'midi',
    // Video
    'mp4', 'avi', 'mov', 'mkv', 'flv', 'wmv', 'webm', 'm4v', '3gp', 'mpeg', 'mpg', 'ts_video',
    // Archives & Compressed
    'zip', 'tar', 'gz', 'tgz', 'bz2', 'tbz2', 'rar', '7z', 'xz', 'txz', 'z', 'cab', 'iso', 'dmg', 'vdi', 'vmdk', 'qcow2',
    // Executables & Binaries & Libraries
    'exe', 'dll', 'so', 'dylib', 'bin', 'app', 'msi', 'pkg', 'deb', 'rpm', 'apk', 'ipa', 'a', 'lib', 'o', 'obj', 'node', 'wasm', 'elf',
    // Compiled Bytecode & Caches
    'pyc', 'pyo', 'pyd', 'class', 'jar', 'war', 'ear', 'elc', 'beam',
    // Fonts
    'woff', 'woff2', 'ttf', 'eot', 'otf', 'fon', 'ttc',
    // Documents & Presentations
    'pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'odt', 'ods', 'odp',
    // Databases & Big Data
    'db', 'sqlite', 'sqlite3', 'db3', 's3db', 'mdb', 'accdb', 'rdb', 'parquet', 'feather', 'arrow', 'h5', 'hdf5',
    // AI / ML Weights
    'pt', 'pth', 'onnx', 'safetensors', 'ckpt', 'gguf', 'pkl', 'pickle', 'joblib',
    // System & Lock
    'ds_store', 'thumbs.db', 'directory', 'lock', 'swp', 'swo',
  ]);

  const LANG_MAP: Record<string, string> = {
    // JavaScript / TypeScript / Web
    ts: 'typescript', tsx: 'typescript', mts: 'typescript', cts: 'typescript',
    js: 'javascript', jsx: 'javascript', mjs: 'javascript', cjs: 'javascript',
    svelte: 'html', vue: 'html', astro: 'html',
    html: 'html', htm: 'html', xhtml: 'html',
    css: 'css', scss: 'scss', sass: 'scss', less: 'less',
    json: 'json', jsonc: 'json', json5: 'json', map: 'json',
    xml: 'xml', svg: 'xml', plist: 'xml', xsd: 'xml', xsl: 'xml', rss: 'xml',
    graphql: 'graphql', gql: 'graphql',
    
    // Systems & Backend
    rs: 'rust',
    py: 'python', pyw: 'python', rpy: 'python', ipy: 'python',
    go: 'go',
    c: 'c', h: 'c',
    cpp: 'cpp', cc: 'cpp', cxx: 'cpp', hpp: 'cpp', hh: 'cpp', hxx: 'cpp', ino: 'cpp',
    cs: 'csharp', csx: 'csharp',
    java: 'java', jav: 'java', jsp: 'java',
    kt: 'kotlin', kts: 'kotlin',
    swift: 'swift',
    php: 'php', phtml: 'php',
    rb: 'ruby', erb: 'ruby', rake: 'ruby', gemspec: 'ruby',
    scala: 'scala', sc: 'scala',
    r: 'r', rmd: 'r',
    dart: 'dart',
    lua: 'lua',
    clj: 'clojure', cljs: 'clojure', edn: 'clojure',
    
    // Config / Markup / Scripts
    yaml: 'yaml', yml: 'yaml',
    toml: 'ini', ini: 'ini', conf: 'ini', config: 'ini', cfg: 'ini', properties: 'ini', env: 'ini',
    md: 'markdown', markdown: 'markdown', mdx: 'markdown', mdown: 'markdown',
    sh: 'shell', bash: 'shell', zsh: 'shell', ksh: 'shell', fish: 'shell', command: 'shell',
    bat: 'bat', cmd: 'bat',
    ps1: 'powershell', psm1: 'powershell',
    sql: 'sql', mysql: 'sql', pgsql: 'sql',
    dockerfile: 'dockerfile', containerfile: 'dockerfile',
    diff: 'diff', patch: 'diff',
    proto: 'protobuf',
    txt: 'plaintext', log: 'plaintext', csv: 'plaintext', tsv: 'plaintext',
  };

  const EXT_COLORS: Record<string, string> = {
    // TypeScript / JS
    ts: '#4da3ff', tsx: '#4da3ff', mts: '#4da3ff', cts: '#4da3ff',
    js: '#f2c96f', jsx: '#f2c96f', mjs: '#f2c96f', cjs: '#f2c96f',
    // Web Frameworks
    svelte: '#ff6f43', vue: '#42b883', astro: '#ff5d01',
    // Python
    py: '#5ec2ff', pyw: '#5ec2ff',
    // Rust
    rs: '#e47770',
    // Go
    go: '#00add8',
    // C / C++ / C#
    c: '#a8b9cc', cpp: '#f34b7d', cc: '#f34b7d', h: '#a8b9cc', hpp: '#f34b7d',
    cs: '#178600',
    // JVM / Mobile
    java: '#b07219', kt: '#a97bff', kts: '#a97bff', swift: '#f05138', dart: '#00b4ab',
    // PHP / Ruby
    php: '#777bb4', rb: '#cc342d',
    // HTML / CSS
    html: '#ff7b72', htm: '#ff7b72',
    css: '#5ce1e6', scss: '#ff79c6', less: '#2b4e7b',
    // Data / Config
    json: '#f2c96f', jsonc: '#f2c96f', json5: '#f2c96f',
    yaml: '#a78bfa', yml: '#a78bfa',
    toml: '#9c4221', ini: '#a19a90', env: '#7cff9e',
    xml: '#ff9e64', svg: '#ffb454',
    md: '#c8a8ff', mdx: '#c8a8ff', markdown: '#c8a8ff',
    // Shell / Scripts
    sh: '#7cff9e', bash: '#7cff9e', zsh: '#7cff9e', bat: '#c1f12e', ps1: '#012456',
    // Database
    sql: '#38bdf8',
    // DevOps
    dockerfile: '#38bdf8',
  };

  function langFromPath(path: string): string {
    const filename = path.split('/').pop()?.toLowerCase() ?? '';
    if (filename === 'dockerfile' || filename.startsWith('dockerfile.') || filename === 'containerfile') {
      return 'dockerfile';
    }
    if (filename === 'makefile' || filename === 'gnumakefile') return 'dockerfile';
    if (filename.startsWith('.env')) return 'ini';
    if (
      filename === '.gitignore' ||
      filename === '.gitattributes' ||
      filename === '.gitmodules' ||
      filename === '.dockerignore' ||
      filename === '.npmignore' ||
      filename === '.editorconfig'
    ) {
      return 'ini';
    }
    const ext = filename.split('.').pop() ?? '';
    return LANG_MAP[ext] ?? 'plaintext';
  }

  function extColorFromPath(path: string): string {
    const filename = path.split('/').pop()?.toLowerCase() ?? '';
    if (filename.startsWith('.env')) return '#7cff9e';
    if (filename === 'dockerfile' || filename.startsWith('dockerfile.')) return '#38bdf8';
    if (filename === 'makefile' || filename === 'gnumakefile') return '#e47770';
    if (filename.startsWith('.git')) return '#f05032';
    const ext = filename.split('.').pop() ?? '';
    return EXT_COLORS[ext] ?? '#7a736b';
  }

  function isBinaryPath(path: string): boolean {
    const filename = path.split('/').pop()?.toLowerCase() ?? '';
    if (filename === '.ds_store' || filename === 'thumbs.db' || filename === 'desktop.ini') return true;
    if (path.includes('__pycache__/') || filename.endsWith('.pyc') || filename.endsWith('.pyo') || filename.endsWith('.pyd')) {
      return true;
    }
    const ext = filename.split('.').pop() ?? '';
    return BINARY_EXTENSIONS.has(ext);
  }

  // ── Tree helpers ──────────────────────────────────────────────
  interface TreeItem {
    kind: 'file' | 'dir';
    rel: string;   // full rel path for files, '' for dirs
    path: string;  // dir path like "src/components" or full rel for files
    name: string;
    depth: number;
    open: boolean;
  }

  type DirNode = { dirs: Map<string, DirNode>; files: string[] };

  // openDirs: only dirs explicitly opened by the user (default = all closed)
  let openDirs = new Set<string>();

  function buildFlatTree(files: string[], open: Set<string>): TreeItem[] {
    const root: DirNode = { dirs: new Map(), files: [] };
    for (const rel of files) {
      const parts = rel.split('/');
      let node = root;
      for (let i = 0; i < parts.length - 1; i++) {
        const p = parts[i];
        if (!node.dirs.has(p)) node.dirs.set(p, { dirs: new Map(), files: [] });
        node = node.dirs.get(p)!;
      }
      node.files.push(rel);
    }
    const result: TreeItem[] = [];
    function walk(node: DirNode, depth: number, prefix: string) {
      for (const [name, child] of [...node.dirs.entries()].sort(([a], [b]) => a.localeCompare(b))) {
        const path = prefix ? `${prefix}/${name}` : name;
        const isOpen = open.has(path);
        result.push({ kind: 'dir', rel: '', path, name, depth, open: isOpen });
        if (isOpen) walk(child, depth + 1, path);
      }
      for (const rel of [...node.files].sort()) {
        result.push({ kind: 'file', rel, path: rel, name: rel.split('/').pop()!, depth, open: false });
      }
    }
    walk(root, 0, '');
    return result;
  }

  function toggleDir(path: string) {
    openDirs = new Set(openDirs);
    if (openDirs.has(path)) openDirs.delete(path);
    else openDirs.add(path);
  }

  // File list state
  let query = '';
  let allFiles: string[] = [];
  let filteredFiles: string[] = [];
  let treeItems: TreeItem[] = [];
  let loadingFiles = false;

  // Editor state
  let openPath: string | null = null;
  let openContent = '';
  let openLanguage = 'plaintext';
  let loadingFile = false;
  let dirty = false;
  let isBinary = false;
  let saving = false;
  let editorComponent: MonacoEditor;
  let pendingPath: string | null = null;

  // Content cache: rel path → file content (avoids re-reading unchanged files)
  const fileCache = new Map<string, string>();

  async function loadFileList() {
    loadingFiles = true;
    try {
      allFiles = await listProjectFiles(cwd);
    } catch {
      allFiles = [];
    } finally {
      loadingFiles = false;
    }
  }

  function fuzzyMatch(str: string, pattern: string): boolean {
    let pi = 0;
    const s = str.toLowerCase();
    const p = pattern.toLowerCase();
    for (let si = 0; si < s.length && pi < p.length; si++) {
      if (s[si] === p[pi]) pi++;
    }
    return pi === p.length;
  }

  let _searchTimer: ReturnType<typeof setTimeout> | null = null;
  let debouncedQuery = '';

  $: {
    if (_searchTimer) clearTimeout(_searchTimer);
    const raw = query;
    _searchTimer = setTimeout(() => {
      debouncedQuery = raw.trim().toLowerCase();
    }, 120);
  }

  $: {
    const q = debouncedQuery;
    if (q) {
      const results: string[] = [];
      for (const f of allFiles) {
        if (fuzzyMatch(f, q)) {
          results.push(f);
          if (results.length >= 100) break;
        }
      }
      filteredFiles = results;
      treeItems = [];
    } else {
      filteredFiles = [];
      treeItems = buildFlatTree(allFiles, openDirs);
    }
  }

  async function openFile(rel: string) {
    if (dirty) {
      pendingPath = rel;
      return;
    }
    await doOpenFile(rel);
  }

  async function doOpenFile(rel: string) {
    openPath = rel;
    openContent = '';
    loadingFile = true;
    dirty = false;
    isBinary = false;
    pendingPath = null;

    if (isBinaryPath(rel)) {
      openContent = '';
      openLanguage = 'plaintext';
      isBinary = true;
      loadingFile = false;
      return;
    }

    // Cache hit: show instantly, skip network round-trip
    if (fileCache.has(rel)) {
      openContent = fileCache.get(rel)!;
      openLanguage = langFromPath(rel);
      loadingFile = false;
      return;
    }

    const full = cwd.replace(/\/$/, '') + '/' + rel;
    try {
      const raw = await readFileContent(full);
      fileCache.set(rel, raw);
      openContent = raw;
      openLanguage = langFromPath(rel);
    } catch (e: any) {
      const errStr = String(e ?? '');
      if (
        errStr.includes('valid UTF-8') ||
        errStr.includes('InvalidData') ||
        errStr.includes('stream did not contain')
      ) {
        openContent = '';
        openLanguage = 'plaintext';
        isBinary = true;
      } else {
        openContent = '';
        openLanguage = 'plaintext';
        toast(`Cannot open ${rel}: ${e}`, 'error');
      }
    } finally {
      loadingFile = false;
    }
  }

  async function saveFile() {
    if (!openPath || isBinary || saving) return;
    saving = true;
    const full = cwd.replace(/\/$/, '') + '/' + openPath;
    try {
      const val = editorComponent?.getValue() ?? openContent;
      await writeFileContent(full, val);
      fileCache.set(openPath, val); // keep cache in sync after save
      editorComponent?.markSaved();
      dirty = false;
      toast('File saved successfully', 'success');
    } catch (e) {
      toast(`Save failed: ${e}`, 'error');
    } finally {
      saving = false;
    }
  }

  function confirmDiscard() {
    dirty = false;
    if (pendingPath) doOpenFile(pendingPath);
    pendingPath = null;
  }

  function cancelDiscard() {
    pendingPath = null;
  }

  $: shortCwd = shortenPath(cwd);
  $: currentFileName = openPath ? openPath.split('/').pop() : '';
  $: currentFileDir = openPath && openPath.includes('/') ? openPath.split('/').slice(0, -1).join('/') : '';
  $: fileExtColor = openPath ? extColorFromPath(openPath) : '#7a736b';

  onMount(() => {
    loadFileList();
  });
</script>

<div class="file-panel">
  <PanelHeader
    title="Files"
    path={shortCwd}
    pathFull={cwd}
    focused={focused}
    onClose={onClose}
  />

  <div class="file-body">
    <!-- Sidebar: file tree -->
    <aside class="sidebar">
      <div class="sidebar-header">
        <div class="sidebar-title-row">
          <span class="sidebar-title">EXPLORER</span>
          <span class="file-count-badge">{query.trim() ? filteredFiles.length : allFiles.length} files</span>
        </div>
        <div class="search-row">
          <Search size={12} class="search-icon" />
          <input
            class="search-input"
            placeholder="Search files…"
            bind:value={query}
            type="text"
            spellcheck={false}
          />
          {#if query}
            <button class="clear-btn" on:click={() => (query = '')} type="button" aria-label="Clear search">
              <X size={11} />
            </button>
          {/if}
        </div>
      </div>

      <div class="file-list" role="listbox" aria-label="Project files">
        {#if loadingFiles}
          <div class="state-msg">
            <div class="spinner-sm"></div>
            <span>Loading files…</span>
          </div>
        {:else if query.trim() ? filteredFiles.length === 0 : treeItems.length === 0}
          <div class="state-msg empty-files">
            <AlertCircle size={14} />
            <span>{query ? 'No matching files' : 'No files found'}</span>
          </div>
        {:else if query.trim()}
          {#each filteredFiles as rel (rel)}
            {@const active = openPath === rel}
            {@const isItemDirty = openPath === rel && dirty}
            {@const itemColor = extColorFromPath(rel)}
            <div
              class="file-item"
              class:active
              class:dirty={isItemDirty}
              role="option"
              aria-selected={active}
              tabindex="0"
              style="--depth: 0"
              on:click={() => openFile(rel)}
              on:keydown={(e) => e.key === 'Enter' && openFile(rel)}
            >
              <span class="file-dot-indicator" style="background-color: {itemColor};"></span>
              <div class="file-info-col">
                <span class="file-name" title={rel}>{rel.split('/').pop()}</span>
                {#if rel.includes('/')}
                  <span class="file-dir">{rel.split('/').slice(0, -1).join('/')}</span>
                {/if}
              </div>
              {#if isItemDirty}<span class="dirty-indicator">●</span>{/if}
            </div>
          {/each}
        {:else}
          {#each treeItems as item (item.kind + ':' + item.path)}
            {#if item.kind === 'dir'}
              <button
                class="tree-dir"
                style="--depth: {item.depth}"
                on:click={() => toggleDir(item.path)}
                type="button"
                aria-expanded={item.open}
              >
                <span class="chevron" class:open={item.open}><ChevronRight size={11} /></span>
                {#if item.open}<FolderOpen size={12} class="dir-icon" />{:else}<Folder size={12} class="dir-icon" />{/if}
                <span class="dir-name">{item.name}</span>
              </button>
            {:else}
              {@const active = openPath === item.rel}
              {@const isItemDirty = openPath === item.rel && dirty}
              {@const itemColor = extColorFromPath(item.rel)}
              <div
                class="file-item"
                class:active
                class:dirty={isItemDirty}
                role="option"
                aria-selected={active}
                tabindex="0"
                style="--depth: {item.depth}"
                on:click={() => openFile(item.rel)}
                on:keydown={(e) => e.key === 'Enter' && openFile(item.rel)}
              >
                <span class="file-dot-indicator" style="background-color: {itemColor};"></span>
                <span class="file-name" title={item.rel}>{item.name}</span>
                {#if isItemDirty}<span class="dirty-indicator">●</span>{/if}
              </div>
            {/if}
          {/each}
        {/if}
      </div>
    </aside>

    <!-- Editor area -->
    <main class="editor-area">
      {#if openPath}
        <!-- Top bar -->
        <header class="editor-topbar">
          <div class="editor-breadcrumb">
            <div class="file-icon-box" style="--ext-color: {fileExtColor};">
              <FileCode size={13} />
            </div>
            <div class="breadcrumb-segments">
              {#if currentFileDir}
                <span class="crumb-dir">{currentFileDir}</span>
                <span class="crumb-sep">/</span>
              {/if}
              <span class="crumb-active-file" title={openPath}>{currentFileName}</span>
            </div>
            {#if dirty}
              <span class="status-pill dirty-pill">
                <span class="status-dot"></span>
                <span>Modified</span>
              </span>
            {:else}
              <span class="status-pill saved-pill">
                <Check size={10} />
                <span>Saved</span>
              </span>
            {/if}
          </div>

          <div class="editor-actions">
            <span class="lang-tag">{openLanguage.toUpperCase()}</span>
            <button
              class="save-btn"
              class:dirty
              class:saving
              disabled={!dirty || saving || isBinary}
              on:click={saveFile}
              type="button"
              title="Save changes (⌘S)"
            >
              {#if saving}
                <div class="btn-spinner"></div>
                <span>Saving…</span>
              {:else}
                <Save size={12} />
                <span>Save</span>
                <kbd class="kbd-hint">⌘S</kbd>
              {/if}
            </button>
          </div>
        </header>

        {#if pendingPath}
          <div class="discard-banner">
            <div class="discard-text">
              <AlertCircle size={14} class="discard-icon" />
              <span>You have unsaved changes. Discard and open <strong>{pendingPath?.split('/').pop()}</strong>?</span>
            </div>
            <div class="discard-btns">
              <button class="discard-yes" on:click={confirmDiscard} type="button">Discard</button>
              <button class="discard-no" on:click={cancelDiscard} type="button">Keep editing</button>
            </div>
          </div>
        {/if}

        {#if loadingFile}
          <div class="editor-loading-state">
            <div class="spinner"></div>
            <span>Opening {openPath}…</span>
          </div>
        {:else if isBinary}
          <div class="binary-warning-state">
            <div class="binary-card">
              <FileText size={32} class="binary-icon" />
              <h3>Binary File</h3>
              <p>This file format cannot be displayed or edited in the code editor.</p>
              <span class="binary-path">{openPath}</span>
            </div>
          </div>
        {:else}
          <div class="editor-wrap">
            <MonacoEditor
              bind:this={editorComponent}
              content={openContent}
              language={openLanguage}
              readOnly={openContent.length > MAX_BINARY_SIZE}
              filePath={openPath}
              on:dirty={(e) => (dirty = e.detail.dirty)}
              on:save={saveFile}
            />
          </div>
        {/if}
      {:else}
        <!-- Clean Empty State -->
        <div class="no-file-state">
          <div class="empty-card">
            <div class="empty-icon-halo">
              <Code2 size={28} />
            </div>
            <h3>Code Editor</h3>
            <p>Select a file from the explorer on the left or use search to edit directly in the app.</p>
            <div class="shortcut-tip">
              <span>Quick save shortcut:</span>
              <kbd>⌘S</kbd>
            </div>
          </div>
        </div>
      {/if}
    </main>
  </div>
</div>

<style>
  .file-panel {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    min-height: 0;
    background: var(--bg);
  }

  .file-body {
    display: flex;
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: var(--bg);
  }

  /* ── Sidebar: File Explorer ── */
  .sidebar {
    display: flex;
    flex-direction: column;
    width: 240px;
    min-width: 200px;
    max-width: 320px;
    flex-shrink: 0;
    border-right: 1px solid var(--bd);
    background: color-mix(in srgb, var(--bg1) 85%, black);
    min-height: 0;
    user-select: none;
  }

  .sidebar-header {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 10px 8px 10px;
    border-bottom: 1px solid var(--bd);
    flex-shrink: 0;
    background: var(--bg-sidebar);
  }

  .sidebar-title-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 2px;
  }

  .sidebar-title {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.08em;
    color: var(--t2);
  }

  .file-count-badge {
    font-size: 9px;
    font-family: var(--mono);
    color: var(--t3);
    background: var(--bg2);
    padding: 1px 6px;
    border-radius: 999px;
    border: 1px solid var(--bd);
  }

  .search-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 5px 8px;
    border-radius: var(--radius-sm, 5px);
    background: var(--bg2);
    border: 1px solid var(--bd);
    transition: border-color 0.15s, box-shadow 0.15s;
  }

  .search-row:focus-within {
    border-color: var(--ac-border);
    box-shadow: 0 0 0 2px var(--ac-d2);
  }

  :global(.search-icon) {
    color: var(--t3);
    flex-shrink: 0;
  }

  .search-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    font-size: 11px;
    color: var(--t0);
    font-family: var(--sans, inherit);
    min-width: 0;
  }

  .search-input::placeholder {
    color: var(--t3);
  }

  .clear-btn {
    border: none;
    background: transparent;
    color: var(--t3);
    cursor: pointer;
    padding: 2px;
    border-radius: 3px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: color 0.12s, background 0.12s;
  }

  .clear-btn:hover {
    color: var(--t0);
    background: var(--bg3);
  }

  .file-list {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 6px 4px;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .tree-dir {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    padding: 3px 8px;
    padding-left: calc(8px + var(--depth, 0) * 14px);
    border: none;
    background: transparent;
    color: var(--t2);
    font-size: 11px;
    font-family: var(--mono);
    cursor: pointer;
    text-align: left;
    border-radius: 3px;
    transition: background 0.1s, color 0.1s;
  }

  .tree-dir:hover {
    background: var(--bg2);
    color: var(--t0);
  }

  .chevron {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    color: var(--t3);
    transition: transform 0.15s;
  }

  .chevron.open {
    transform: rotate(90deg);
  }

  :global(.dir-icon) {
    flex-shrink: 0;
    color: #f2c96f;
    opacity: 0.8;
  }

  .dir-name {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 500;
  }

  .file-item {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 8px;
    padding-left: calc(8px + var(--depth, 0) * 14px);
    border-radius: 4px;
    cursor: pointer;
    min-width: 0;
    transition: background 0.1s, border-color 0.1s;
    border-left: 2px solid transparent;
  }

  .file-item:hover {
    background: var(--bg2);
  }

  .file-item.active {
    background: var(--bg2);
    border-left: 2px solid var(--ac);
  }

  .file-dot-indicator {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    opacity: 0.85;
  }

  .file-info-col {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
    gap: 1px;
  }

  .file-name {
    font-size: 11px;
    font-family: var(--mono);
    color: var(--t1);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    line-height: 1.3;
  }

  .file-item.active .file-name {
    color: var(--t0);
    font-weight: 500;
  }

  .file-dir {
    font-size: 9px;
    color: var(--t3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    line-height: 1.2;
  }

  .dirty-indicator {
    color: #f2c96f;
    font-size: 10px;
    flex-shrink: 0;
    margin-left: auto;
    animation: pulse 1.8s infinite ease-in-out;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.5; transform: scale(0.85); }
  }

  .state-msg {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 16px 12px;
    font-size: 11px;
    color: var(--t3);
    font-family: var(--mono);
  }

  .empty-files {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 6px;
    padding: 24px 12px;
  }

  /* ── Editor Area ── */
  .editor-area {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    height: 100%;
    background: var(--bg);
  }

  .editor-topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    height: 38px;
    padding: 0 14px;
    border-bottom: 1px solid var(--bd);
    background: color-mix(in srgb, var(--bg) 95%, white);
    flex-shrink: 0;
    user-select: none;
    min-width: 0;
  }

  .editor-breadcrumb {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    flex: 1;
    overflow: hidden;
  }

  .file-icon-box {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--ext-color) 12%, transparent);
    color: var(--ext-color);
    flex-shrink: 0;
  }

  .breadcrumb-segments {
    display: flex;
    align-items: baseline;
    gap: 5px;
    min-width: 0;
    overflow: hidden;
    font-family: var(--mono);
    font-size: 11px;
  }

  .crumb-dir {
    color: var(--t3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .crumb-sep {
    color: var(--t3);
    opacity: 0.6;
  }

  .crumb-active-file {
    color: var(--t0);
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .status-pill {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 9px;
    font-family: var(--mono);
    padding: 2px 7px;
    border-radius: 999px;
    flex-shrink: 0;
    line-height: 1.3;
  }

  .dirty-pill {
    background: rgba(242, 201, 111, 0.12);
    color: #f2c96f;
    border: 1px solid rgba(242, 201, 111, 0.28);
  }

  .status-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background-color: #f2c96f;
  }

  .saved-pill {
    background: var(--ac-d2);
    color: var(--ac);
    border: 1px solid var(--ac-border);
    opacity: 0.8;
  }

  .editor-actions {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-shrink: 0;
  }

  .lang-tag {
    font-size: 9px;
    font-family: var(--mono);
    font-weight: 600;
    color: var(--t3);
    background: var(--bg2);
    padding: 3px 7px;
    border-radius: 4px;
    border: 1px solid var(--bd);
    letter-spacing: 0.05em;
  }

  .save-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    font-size: 11px;
    font-weight: 500;
    border-radius: var(--radius-sm, 5px);
    border: 1px solid var(--bd);
    background: var(--bg2);
    color: var(--t2);
    cursor: pointer;
    transition: all 0.15s ease;
    user-select: none;
  }

  .save-btn.dirty {
    background: var(--ac);
    color: #000;
    border-color: var(--ac);
    font-weight: 600;
    box-shadow: 0 1px 4px rgba(124, 255, 158, 0.25);
  }

  .save-btn.dirty:hover:not(:disabled) {
    filter: brightness(1.08);
    transform: translateY(-0.5px);
  }

  .save-btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .kbd-hint {
    font-size: 9px;
    font-family: var(--mono);
    padding: 1px 4px;
    border-radius: 3px;
    background: rgba(0, 0, 0, 0.15);
    color: currentColor;
    opacity: 0.8;
  }

  .discard-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 14px;
    background: rgba(242, 201, 111, 0.08);
    border-bottom: 1px solid rgba(242, 201, 111, 0.25);
    font-size: 11px;
    color: var(--t0);
    flex-shrink: 0;
  }

  .discard-text {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    flex: 1;
  }

  :global(.discard-icon) {
    color: #f2c96f;
    flex-shrink: 0;
  }

  .discard-btns {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .discard-yes,
  .discard-no {
    padding: 3px 10px;
    border-radius: var(--radius-sm, 4px);
    font-size: 10px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.12s;
  }

  .discard-yes {
    background: #e47770;
    color: #fff;
    border: 1px solid transparent;
  }

  .discard-yes:hover {
    background: #c96860;
  }

  .discard-no {
    background: var(--bg2);
    color: var(--t1);
    border: 1px solid var(--bd);
  }

  .discard-no:hover {
    background: var(--bg3);
    color: var(--t0);
  }

  /* ── Monaco Editor Wrap ── */
  .editor-wrap {
    flex: 1;
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    position: relative;
    background: var(--bg);
  }

  .editor-loading-state {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    font-size: 12px;
    color: var(--t2);
    font-family: var(--mono);
  }

  /* ── Empty State ── */
  .no-file-state {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background: var(--bg);
  }

  .empty-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    max-width: 340px;
    padding: 32px 24px;
    border-radius: 12px;
    background: color-mix(in srgb, var(--bg1) 60%, transparent);
    border: 1px solid var(--bd);
  }

  .empty-icon-halo {
    width: 54px;
    height: 54px;
    border-radius: 50%;
    background: var(--ac-d2);
    border: 1px solid var(--ac-border);
    color: var(--ac);
    display: flex;
    align-items: center;
    justify-content: center;
    margin-bottom: 14px;
  }

  .empty-card h3 {
    margin: 0 0 6px 0;
    font-size: 14px;
    font-weight: 600;
    color: var(--t0);
  }

  .empty-card p {
    margin: 0 0 16px 0;
    font-size: 11px;
    color: var(--t2);
    line-height: 1.5;
  }

  .shortcut-tip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    border-radius: 999px;
    background: var(--bg2);
    border: 1px solid var(--bd);
    font-size: 10px;
    color: var(--t3);
  }

  .shortcut-tip kbd {
    font-family: var(--mono);
    color: var(--t1);
    font-weight: 600;
  }

  /* ── Binary Warning State ── */
  .binary-warning-state {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
  }

  .binary-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    max-width: 320px;
    padding: 24px;
    border-radius: 8px;
    background: var(--bg1);
    border: 1px solid var(--bd);
  }

  :global(.binary-icon) {
    color: var(--t3);
    margin-bottom: 10px;
  }

  .binary-card h3 {
    margin: 0 0 6px 0;
    font-size: 13px;
    color: var(--t0);
  }

  .binary-card p {
    margin: 0 0 12px 0;
    font-size: 11px;
    color: var(--t2);
    line-height: 1.4;
  }

  .binary-path {
    font-size: 10px;
    font-family: var(--mono);
    color: var(--t3);
    word-break: break-all;
  }

  /* ── Spinners ── */
  .spinner {
    width: 20px;
    height: 20px;
    border: 2px solid var(--bd1);
    border-top-color: var(--ac);
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  .spinner-sm {
    width: 12px;
    height: 12px;
    border: 2px solid var(--bd1);
    border-top-color: var(--ac);
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  .btn-spinner {
    width: 10px;
    height: 10px;
    border: 1.5px solid currentColor;
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
