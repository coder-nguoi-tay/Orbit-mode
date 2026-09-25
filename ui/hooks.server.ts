import type { HandleServerError } from '@sveltejs/kit';

export const handleError: HandleServerError = ({ error, event, status, message }) => {
  console.error('[SvelteKit Server Error]', status, message, error, event);
  return {
    message: error instanceof Error ? error.message : message || 'Server Error',
    stack: error instanceof Error ? error.stack : undefined,
    status,
  };
};
