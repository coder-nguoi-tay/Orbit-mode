import type { Pipeline, PipelineConfig } from '../types';
import { invoke } from './invoke';

export async function createPipeline(opts: {
  name: string;
  userRequest: string;
  worktreePath: string;
  config: PipelineConfig;
}): Promise<Pipeline> {
  return await invoke('create_pipeline', {
    name: opts.name,
    userRequest: opts.userRequest,
    worktreePath: opts.worktreePath,
    config: opts.config,
  });
}

export async function listPipelines(): Promise<Pipeline[]> {
  return await invoke('list_pipelines');
}

export async function getPipeline(pipelineId: number): Promise<Pipeline | null> {
  return await invoke('get_pipeline', { pipelineId });
}

export async function cancelPipeline(pipelineId: number): Promise<Pipeline> {
  return await invoke('cancel_pipeline', { pipelineId });
}

export async function setPipelineStatus(pipelineId: number, status: string): Promise<Pipeline> {
  return await invoke('set_pipeline_status', { pipelineId, status });
}

export function defaultPipelineConfig(
  plannerProvider = 'claude-code',
  plannerModel = 'auto',
  devProvider = 'claude-code',
  devModel = 'auto'
): PipelineConfig {
  return {
    planner: { provider: plannerProvider, model: plannerModel },
    developer: { provider: devProvider, model: devModel },
    reviewer: { provider: devProvider, model: devModel },
    tester: { provider: devProvider, model: devModel },
    limits: {
      maxPlanRevisions: 2,
      maxReviewLoops: 3,
      maxTestFixLoops: 3,
      maxTotalAgentRuns: 12,
    },
    requireFinalReview: true,
    requireTests: true,
    qualityCommands: [],
  };
}
