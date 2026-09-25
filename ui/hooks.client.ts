import type { HandleClientError } from '@sveltejs/kit';

export const handleError: HandleClientError = ({ error, event, status, message }) => {
  console.error('[SvelteKit Client Error]', status, message, error, event);
  return {
    message: error instanceof Error ? error.message : message || 'Client Error',
    stack: error instanceof Error ? error.stack : undefined,
    status,
  };
};
