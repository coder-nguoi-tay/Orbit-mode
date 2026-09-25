import { describe, it, expect, beforeEach, vi } from 'vitest';
import { render, fireEvent, cleanup } from '@testing-library/svelte';

// --- Mocks ---

const {
  mockActivePipelineIdStore,
  mockWorkspaceStore,
  mockAssignSession,
  mockUpdatePipelineStatus,
  mockCancelPipeline,
} = vi.hoisted(() => {
  function createStore<T>(initial: T) {
    let val = initial;
    const subs = new Set<(v: T) => void>();
    return {
      subscribe: (fn: (v: T) => void) => {
        subs.add(fn);
        fn(val);
        return () => subs.delete(fn);
      },
      set: (v: T) => {
        val = v;
        subs.forEach((fn) => fn(val));
      },
      update: (updater: (v: T) => T) => {
        val = updater(val);
        subs.forEach((fn) => fn(val));
      },
    };
  }
  return {
    mockActivePipelineIdStore: createStore<number | null>(null),
    mockWorkspaceStore: createStore<any>({ focusedPaneId: 'pane-1' }),
    mockAssignSession: vi.fn(),
    mockUpdatePipelineStatus: vi.fn(),
    mockCancelPipeline: vi.fn().mockResolvedValue({ id: 1, status: 'cancelled', steps: [] }),
  };
});

vi.mock('$lib/stores/pipelines', () => ({
  activePipelineId: mockActivePipelineIdStore,
  updatePipelineStatus: mockUpdatePipelineStatus,
}));

vi.mock('$lib/stores/workspace', () => ({
  workspace: mockWorkspaceStore,
  assignSession: mockAssignSession,
}));

vi.mock('$lib/tauri/pipelines', () => ({
  cancelPipeline: mockCancelPipeline,
}));

// --- Helpers ---

import PipelinePanel from './PipelinePanel.svelte';
import type { Pipeline, PipelineStep } from '../lib/types';

function makeStep(overrides: Partial<PipelineStep> = {}): PipelineStep {
  return {
    id: 1,
    pipelineId: 1,
    role: 'planner',
    status: 'pending',
    providerId: 'claude-code',
    model: 'auto',
    attempt: 1,
    startedAt: null,
    completedAt: null,
    error: null,
    sessionId: null,
    ...overrides,
  };
}

function makePipeline(overrides: Partial<Pipeline> = {}): Pipeline {
  return {
    id: 1,
    projectId: null,
    name: 'My Pipeline',
    userRequest: 'Build login page',
    worktreePath: '/repo',
    status: 'created',
    config: {
      planner: { provider: 'claude-code', model: 'auto' },
      developer: { provider: 'claude-code', model: 'auto' },
      reviewer: { provider: 'claude-code', model: 'auto' },
      tester: { provider: 'claude-code', model: 'auto' },
      limits: { maxPlanRevisions: 2, maxReviewLoops: 3, maxTestFixLoops: 3, maxTotalAgentRuns: 12 },
      requireFinalReview: false,
      requireTests: false,
      qualityCommands: [],
    },
    baselineGitHead: null,
    reviewLoops: 0,
    testLoops: 0,
    planRevisions: 0,
    totalAgentRuns: 0,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    completedAt: null,
    steps: [],
    ...overrides,
  };
}

beforeEach(() => {
  cleanup();
  vi.clearAllMocks();
  mockActivePipelineIdStore.set(null);
  mockWorkspaceStore.set({ focusedPaneId: 'pane-1' });
});

describe('PipelinePanel', () => {
  it('renders pipeline name and task', () => {
    const { getByText } = render(PipelinePanel, { props: { pipeline: makePipeline() } });
    expect(getByText('My Pipeline')).toBeTruthy();
    expect(getByText('Build login page')).toBeTruthy();
  });

  it('shows Cancel button for non-terminal pipeline', () => {
    const { getByText } = render(PipelinePanel, {
      props: { pipeline: makePipeline({ status: 'planning' }) },
    });
    expect(getByText('Cancel')).toBeTruthy();
  });

  it('hides Cancel button for completed pipeline', () => {
    const { queryByText } = render(PipelinePanel, {
      props: { pipeline: makePipeline({ status: 'completed' }) },
    });
    expect(queryByText('Cancel')).toBeNull();
  });

  it('hides Cancel button for cancelled pipeline', () => {
    const { queryByText } = render(PipelinePanel, {
      props: { pipeline: makePipeline({ status: 'cancelled' }) },
    });
    expect(queryByText('Cancel')).toBeNull();
  });

  it('shows View button when step has a sessionId', () => {
    const step = makeStep({ sessionId: 42 });
    const { getByText } = render(PipelinePanel, {
      props: { pipeline: makePipeline({ steps: [step] }) },
    });
    expect(getByText('View')).toBeTruthy();
  });

  it('hides View button when step has no sessionId', () => {
    const step = makeStep({ sessionId: null });
    const { queryByText } = render(PipelinePanel, {
      props: { pipeline: makePipeline({ steps: [step] }) },
    });
    expect(queryByText('View')).toBeNull();
  });

  it('clicking View closes pipeline panel and opens session', async () => {
    mockActivePipelineIdStore.set(1);
    const step = makeStep({ sessionId: 42 });
    const { getByText } = render(PipelinePanel, {
      props: { pipeline: makePipeline({ steps: [step] }) },
    });
    await fireEvent.click(getByText('View'));
    // activePipelineId should be cleared
    let id: number | null = 99;
    mockActivePipelineIdStore.subscribe((v) => (id = v));
    expect(id).toBeNull();
    // assignSession called with the step's session id
    expect(mockAssignSession).toHaveBeenCalledWith('pane-1', 42);
  });

  it('clicking Cancel calls cancelPipeline and updates store', async () => {
    const pipeline = makePipeline({ status: 'planning' });
    const { getByText } = render(PipelinePanel, { props: { pipeline } });
    await fireEvent.click(getByText('Cancel'));
    expect(mockCancelPipeline).toHaveBeenCalledWith(1);
  });

  it('renders all steps', () => {
    const steps = [makeStep({ id: 1, role: 'planner' }), makeStep({ id: 2, role: 'developer' })];
    const { container } = render(PipelinePanel, {
      props: { pipeline: makePipeline({ steps }) },
    });
    // Two step rows rendered
    const rows = container.querySelectorAll('.step-row');
    expect(rows.length).toBe(2);
  });
});
