import { expect, it, vi } from 'vitest';
import { render, waitFor, fireEvent, cleanup } from '@testing-library/svelte';
import { tick } from 'svelte';

const editorState = vi.hoisted(() => {
  let value = '';
  let onChange = () => {};
  const setValue = vi.fn((nextValue: string) => {
    value = nextValue;
    onChange();
  });
  return {
    model: {
      getValue: () => value,
      setValue,
      onDidChangeContent: (listener: () => void) => {
        onChange = listener;
        return { dispose: () => { onChange = () => {}; } };
      },
    },
    setValue,
    initialize: (initialValue: string) => { value = initialValue; },
  };
});

vi.mock('monaco-editor', () => ({
  KeyMod: { CtrlCmd: 1 },
  KeyCode: { KeyS: 1 },
  editor: {
    create: (_host: Element, options: { value: string }) => {
      editorState.initialize(options.value);
      return {
        getModel: () => editorState.model,
        addAction: () => {},
        updateOptions: () => {},
        layout: () => {},
        dispose: () => {},
      };
    },
    defineTheme: () => {},
    setTheme: () => {},
    setModelLanguage: () => {},
  },
}));

const writeFileContent = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));
const readFileContent = vi.hoisted(() => vi.fn().mockResolvedValue('old'));
const listProjectFiles = vi.hoisted(() => vi.fn().mockResolvedValue(['sample.ts']));
vi.mock('../lib/tauri/files', () => ({ readFileContent }));
vi.mock('../lib/tauri/git', () => ({ writeFileContent }));
vi.mock('../lib/tauri/projects', () => ({ listProjectFiles }));

import MonacoEditor from './MonacoEditor.svelte';
import FileEditorPanel from './FileEditorPanel.svelte';

it('keeps saved content and edits typed while a save completes', async () => {
  const rendered = render(MonacoEditor, { props: { content: 'old', language: 'plaintext' } });
  await waitFor(() => expect(rendered.container.textContent).not.toContain('Loading editor'));

  editorState.model.setValue('saved');
  await tick();
  (rendered.component as { markSaved: (content: string) => void }).markSaved('saved');
  await rendered.rerender({ content: 'saved', language: 'plaintext' });
  expect(editorState.model.getValue()).toBe('saved');

  editorState.model.setValue('saved plus later edits');
  await tick();
  (rendered.component as { markSaved: (content: string) => void }).markSaved('saved');
  await rendered.rerender({ content: 'saved', language: 'plaintext' });
  expect(editorState.model.getValue()).toBe('saved plus later edits');
  cleanup();
});

it('does not restore the old file after Save', async () => {
  writeFileContent.mockClear();
  const rendered = render(FileEditorPanel, { props: { cwd: '/project' } });
  await fireEvent.click(await rendered.findByText('sample.ts'));
  await waitFor(() => expect(editorState.model.getValue()).toBe('old'));
  await tick();

  editorState.model.setValue('new');
  await waitFor(() => expect(rendered.container.textContent).toContain('Modified'));
  await fireEvent.click(rendered.getByRole('button', { name: /Save/ }));
  await waitFor(() => expect(writeFileContent).toHaveBeenCalledWith('/project/sample.ts', 'new'));
  await waitFor(() => expect(rendered.container.textContent).toContain('Saved'));
  expect(editorState.model.getValue()).toBe('new');
  cleanup();
});

it('keeps edits made while the file write is pending', async () => {
  let completeWrite: (() => void) | undefined;
  writeFileContent.mockImplementationOnce(() => new Promise<void>((resolve) => {
    completeWrite = resolve;
  }));
  const rendered = render(FileEditorPanel, { props: { cwd: '/project' } });
  await fireEvent.click(await rendered.findByText('sample.ts'));
  await waitFor(() => expect(editorState.model.getValue()).toBe('old'));
  await tick();

  editorState.model.setValue('first edit');
  await waitFor(() => expect(rendered.container.textContent).toContain('Modified'));
  await fireEvent.click(rendered.getByRole('button', { name: /Save/ }));
  await waitFor(() => expect(completeWrite).toBeDefined());
  editorState.model.setValue('first edit plus more');
  completeWrite?.();

  await waitFor(() => expect(rendered.container.textContent).not.toContain('Saving…'));
  await waitFor(() => expect(rendered.container.textContent).toContain('Modified'));
  expect(editorState.model.getValue()).toBe('first edit plus more');
  cleanup();
});

it('ignores an older file read after the user opens another file', async () => {
  let finishFirstRead: ((content: string) => void) | undefined;
  listProjectFiles.mockResolvedValueOnce(['sample.ts', 'other.ts']);
  readFileContent.mockImplementationOnce(() => new Promise<string>((resolve) => {
    finishFirstRead = resolve;
  })).mockResolvedValueOnce('other content');

  const rendered = render(FileEditorPanel, { props: { cwd: '/project' } });
  await fireEvent.click(await rendered.findByText('sample.ts'));
  await fireEvent.click(rendered.getByText('other.ts'));
  await waitFor(() => expect(editorState.model.getValue()).toBe('other content'));

  finishFirstRead?.('stale content');
  await tick();
  expect(editorState.model.getValue()).toBe('other content');
  cleanup();
});
