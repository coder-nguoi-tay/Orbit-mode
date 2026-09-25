import { writable, derived } from 'svelte/store';
import type { Pipeline, PipelineStep, PipelineStatus } from '../types';

export const pipelines = writable<Pipeline[]>([]);
export const activePipelineId = writable<number | null>(null);

export const activePipeline = derived(
  [pipelines, activePipelineId],
  ([$pipelines, $id]) => $pipelines.find((p) => p.id === $id) ?? null
);

export function upsertPipeline(p: Pipeline) {
  pipelines.update((list) => {
    const idx = list.findIndex((x) => x.id === p.id);
    if (idx >= 0) {
      const next = [...list];
      next[idx] = p;
      return next;
    }
    return [p, ...list];
  });
}

export function updatePipelineStatus(id: number, status: PipelineStatus, steps?: PipelineStep[]) {
  pipelines.update((list) =>
    list.map((p) => (p.id === id ? { ...p, status, ...(steps ? { steps } : {}) } : p))
  );
}

export function openPipeline(id: number) {
  activePipelineId.set(id);
}
