import { useEffect, useRef, useState } from 'react';

//
// Typewriter reveal for streaming text.
//
// Given a growing `content` string, returns the prefix that should be
// displayed right now. A rAF-driven loop advances the revealed length
// toward `content.length` with backlog-proportional speed, so a fast
// burst doesn't visibly lag while a slow stream still types naturally.
//
// When `active` transitions false→true, the reveal resets to zero (a
// new streaming turn begins). When it transitions true→false, the
// tail snaps to full so nothing is hidden on completion.
//

export function useTypewriter(content: string, active: boolean): string {
  const [, tick] = useState(0);
  const revealedRef = useRef(0);
  const contentRef = useRef(content);
  const activeRef = useRef(active);
  const rafRef = useRef<number | null>(null);

  contentRef.current = content;

  useEffect(() => {
    const wasActive = activeRef.current;
    activeRef.current = active;

    if (active && !wasActive) {
      revealedRef.current = 0;
      tick((t) => t + 1);
    }
    if (!active && wasActive) {
      revealedRef.current = contentRef.current.length;
      tick((t) => t + 1);
    }
  }, [active]);

  useEffect(() => {
    if (!active) return;

    let cancelled = false;
    let last = performance.now();

    const step = (now: number) => {
      if (cancelled) return;
      const elapsed = now - last;
      last = now;

      const target = contentRef.current.length;
      const gap = target - revealedRef.current;

      if (gap > 0) {
        //
        // Baseline ~120 chars/sec; speed up when backlog is large so a
        // big chunk doesn't leave the reveal trailing far behind.
        //
        const perSec = gap > 400 ? 1600 : gap > 160 ? 700 : gap > 50 ? 300 : 120;
        const advance = Math.max(1, Math.round((perSec * elapsed) / 1000));
        revealedRef.current = Math.min(target, revealedRef.current + advance);
        tick((t) => t + 1);
      }

      rafRef.current = requestAnimationFrame(step);
    };

    rafRef.current = requestAnimationFrame(step);
    return () => {
      cancelled = true;
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
    };
  }, [active]);

  if (!active) return content;
  return content.slice(0, revealedRef.current);
}
