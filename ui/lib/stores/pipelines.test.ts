import { get } from 'svelte/store';
import { describe, it, expect, beforeEach } from 'vitest';
import {
  pipelines,
  activePipelineId,
  activePipeline,
  upsertPipeline,
  updatePipelineStatus,
  openPipeline,
} from './pipelines';
import type { Pipeline } from '../types';

function makePipeline(id: number, name = `Pipeline ${id}`): Pipeline {
  return {
    id,
    projectId: null,
    name,
    userRequest: 'Do something',
    worktreePath: '/repo',
    status: 'created',
    config: {
      planner: { provider: 'claude-code', model: 'auto' },
      developer: { provider: 'claude-code', model: 'auto' },
      reviewer: { provider: 'claude-code', model: 'auto' },
      tester: { provider: 'claude-code', model: 'auto' },
      limits: { maxPlanRevisions: 2, maxReviewLoops: 3, maxTestFixLoops: 3, maxTotalAgentRuns: 12 },
      requireFinalReview: true,
      requireTests: true,
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
  };
}

beforeEach(() => {
  pipelines.set([]);
  activePipelineId.set(null);
});

describe('pipelines store', () => {
  it('upsertPipeline adds new pipeline', () => {
    upsertPipeline(makePipeline(1));
    expect(get(pipelines)).toHaveLength(1);
    expect(get(pipelines)[0].id).toBe(1);
  });

  it('upsertPipeline updates existing pipeline in place', () => {
    upsertPipeline(makePipeline(1, 'original'));
    upsertPipeline(makePipeline(1, 'updated'));
    const list = get(pipelines);
    expect(list).toHaveLength(1);
    expect(list[0].name).toBe('updated');
  });

  it('upsertPipeline prepends new pipelines', () => {
    upsertPipeline(makePipeline(1));
    upsertPipeline(makePipeline(2));
    // second upsert is a new pipeline → prepended
    expect(get(pipelines)[0].id).toBe(2);
  });

  it('updatePipelineStatus changes status', () => {
    upsertPipeline(makePipeline(1));
    updatePipelineStatus(1, 'planning');
    expect(get(pipelines)[0].status).toBe('planning');
  });

  it('updatePipelineStatus with steps replaces steps', () => {
    const p = makePipeline(1);
    upsertPipeline(p);
    const fakeStep = {
      id: 10,
      pipelineId: 1,
      role: 'planner' as const,
      status: 'running' as const,
      providerId: 'claude-code',
      model: 'auto',
      attempt: 1,
      startedAt: null,
      completedAt: null,
      error: null,
      sessionId: null,
    };
    updatePipelineStatus(1, 'planning', [fakeStep]);
    const updated = get(pipelines)[0];
    expect(updated.status).toBe('planning');
    expect(updated.steps).toHaveLength(1);
    expect(updated.steps[0].role).toBe('planner');
  });

  it('activePipeline derived store returns correct pipeline', () => {
    upsertPipeline(makePipeline(1));
    upsertPipeline(makePipeline(2));
    openPipeline(2);
    expect(get(activePipeline)?.id).toBe(2);
  });

  it('activePipeline is null when no id selected', () => {
    upsertPipeline(makePipeline(1));
    expect(get(activePipeline)).toBeNull();
  });

  it('openPipeline sets activePipelineId', () => {
    openPipeline(42);
    expect(get(activePipelineId)).toBe(42);
  });
});
