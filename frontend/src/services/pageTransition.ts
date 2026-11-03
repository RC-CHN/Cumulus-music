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

interface PageTransitionSpec {
  name: string;
  deadlineMs: number;
}

// The deadline also covers a compositor that stops delivering frames before
// a transition can start. It is independent of the CSS duration and rAF.
const pageTransition = { name: 'page', deadlineMs: 450 };

export function createPageTransition(
  reduced: () => boolean,
  clock = browserClock,
  spec: PageTransitionSpec = pageTransition,
) {
  const pending = new Map<Element, End>();

  function cancel(element: Element) {
    pending.get(element)?.();
  }

  function animate(element: Element, done: End, phase: Phase) {
    cancel(element);
    if (reduced()) {
      // For out-in, finish after Vue commits the placeholder. Re-entering
      // the component update from a synchronous leave can leave it mounted
      // on the old branch even though no animation was requested.
      const cleanup = () => pending.delete(element);
      pending.set(element, cleanup);
      queueMicrotask(() => {
        if (pending.get(element) !== cleanup) return;
        cleanup();
        done();
      });
      return;
    }

    const from = `${spec.name}-${phase}-from`;
    const active = `${spec.name}-${phase}-active`;
    const to = `${spec.name}-${phase}-to`;
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
    timer = clock.deadline(finish, spec.deadlineMs);
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
