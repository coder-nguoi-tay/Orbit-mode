// Component-test setup.
//
// Svelte 5 transitions drive animation through the Web Animations API
// (element.animate). happy-dom does not implement it, so any element carrying a
// transition directive throws "element.animate is not a function". Provide a
// minimal no-op Animation so transitions resolve instantly in tests (the motion
// itself is exercised in the browser, not here).
if (typeof Element !== 'undefined' && !Element.prototype.animate) {
  Element.prototype.animate = function animate() {
    return {
      cancel() {},
      finish() {},
      play() {},
      pause() {},
      reverse() {},
      addEventListener() {},
      removeEventListener() {},
      onfinish: null,
      oncancel: null,
      currentTime: 0,
      playState: 'finished',
      finished: Promise.resolve(),
    } as unknown as Animation;
  };
}

// Node 25+ exposes an unconfigured globalThis.localStorage object.
// Ensure a working Storage implementation is attached.
if (typeof window !== 'undefined') {
  let store: Record<string, string> = {};
  const storageMock = {
    getItem: (key: string) => store[key] ?? null,
    setItem: (key: string, value: string) => {
      store[key] = String(value);
    },
    removeItem: (key: string) => {
      delete store[key];
    },
    clear: () => {
      store = {};
    },
    get length() {
      return Object.keys(store).length;
    },
    key: (index: number) => Object.keys(store)[index] ?? null,
  };
  Object.defineProperty(window, 'localStorage', {
    value: storageMock,
    writable: true,
  });
  Object.defineProperty(globalThis, 'localStorage', {
    value: storageMock,
    writable: true,
  });
}
