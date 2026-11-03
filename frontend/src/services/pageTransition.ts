type Phase = 'enter' | 'leave';
type End = () => void;

interface PageTransitionClock {
  frame(run: End): number;
  cancelFrame(id: number): void;
  deadline(run: End, delay: number): number;
  cancelDeadline(id: number): void;
}

const browserClock: PageTransitionClock = {
  frame: (run) => window.requestAnimationFrame(run),
  cancelFrame: (id) => window.cancelAnimationFrame(id),
  deadline: (run, delay) => window.setTimeout(run, delay),
  cancelDeadline: (id) => window.clearTimeout(id),
};

// Existing page transitions are at most 220ms. The deadline also covers a
// compositor that stops delivering frames before the transition can start.
const maxPageTransitionMs = 450;

export function createPageTransition(reduced: () => boolean, clock = browserClock) {
  const pending = new Map<Element, End>();

  function cancel(element: Element) {
    pending.get(element)?.();
  }

  function animate(element: Element, done: End, phase: Phase) {
    cancel(element);
    if (reduced()) {
      done();
      return;
    }

    const from = `page-${phase}-from`;
    const active = `page-${phase}-active`;
    const to = `page-${phase}-to`;
    let frame: number | undefined;
    let timer: number | undefined;
    let started = false;

    function cleanup() {
      if (pending.get(element) !== cleanup) return;
      pending.delete(element);
      if (frame !== undefined) clock.cancelFrame(frame);
      if (timer !== undefined) clock.cancelDeadline(timer);
      element.removeEventListener('transitionend', transitioned);
      element.classList.remove(from, active, to);
    }
    function finish() {
      if (pending.get(element) !== cleanup) return;
      cleanup();
      done();
    }
    function transitioned(event: Event) {
      // Descendant transitions and the shorter enter opacity transition
      // cannot complete the containing page's transform.
      if (
        started &&
        event.target === element &&
        (event as TransitionEvent).propertyName === 'transform'
      )
        finish();
    }

    pending.set(element, cleanup);
    element.classList.add(from, active);
    element.addEventListener('transitionend', transitioned);
    // Flush the starting style before applying the destination on a frame.
    void element.getBoundingClientRect();
    timer = clock.deadline(finish, maxPageTransitionMs);
    frame = clock.frame(() => {
      if (pending.get(element) !== cleanup) return;
      started = true;
      element.classList.remove(from);
      if (phase === 'leave') element.classList.add(to);
    });
  }

  return {
    enter: (element: Element, done: End) => animate(element, done, 'enter'),
    leave: (element: Element, done: End) => animate(element, done, 'leave'),
    cancel,
    dispose: () => {
      for (const cleanup of pending.values()) cleanup();
    },
  };
}
