// Dock panels are sized by Dockview, not by the window, and neither React nor
// Pixi tracks that on its own:
//
// - `<Application width height>` only sizes the renderer when it mounts —
//   @pixi/react forwards those props into the canvas root and never resizes.
// - Pixi's own `resizeTo` only listens for *window* resizes.
//
// So a panel that draws in its own pixel space has to measure its element and
// tell the renderer, otherwise the canvas keeps its first size: the rest of the
// panel stays unpainted, and any DOM overlay that hit-tests in unscaled units
// no longer lines up with the drawing.
import { useEffect, useRef, useState } from 'react';
import { useTheme } from './palette.js';

/** Tracks an element's content box; returns a ref to attach and the size. */
export function useElementSize(defaultSize = { width: 900, height: 420 }) {
  const ref = useRef(null);
  const [size, setSize] = useState(defaultSize);
  useEffect(() => {
    const host = ref.current;
    if (!host) return undefined;
    const measure = () => {
      const { width, height } = host.getBoundingClientRect();
      if (width <= 0 || height <= 0) return;
      const next = { width: Math.round(width), height: Math.round(height) };
      setSize(current => current.width === next.width && current.height === next.height ? current : next);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(host);
    return () => observer.disconnect();
  }, []);
  return [ref, size];
}

/**
 * Resizes a Pixi renderer to the panel's measured size. The Pixi app boots
 * asynchronously, so the resize retries on the next frames until the renderer
 * exists.
 */
export function useRendererSize(ref, width, height) {
  useEffect(() => {
    if (width <= 0 || height <= 0) return undefined;
    const resize = () => {
      const app = ref.current?.getApplication?.();
      if (!app?.renderer) return false;
      if (app.renderer.width !== width || app.renderer.height !== height) app.renderer.resize(width, height);
      return true;
    };
    if (resize()) return undefined;
    let frame = 0;
    let tries = 0;
    const retry = () => {
      if (resize() || ++tries > 60) return;
      frame = requestAnimationFrame(retry);
    };
    frame = requestAnimationFrame(retry);
    return () => cancelAnimationFrame(frame);
  }, [ref, width, height]);
}

/**
 * Stops Pixi's ticker and draws once per React commit instead.
 *
 * Pixi renders the whole scene on every ticker frame. These panels are static:
 * nothing on screen changes unless React state changed, so the idle ticker spent
 * ~35 full-scene rasterisations per second producing pixels identical to the
 * previous frame. Measured on this machine, where WebGL falls back to the
 * SwiftShader software rasteriser, that was ~9 CPU cores for a page nobody was
 * touching; with the ticker stopped the same page costs ~0.7.
 *
 * The draw is deferred to the next animation frame because `@pixi/react` applies
 * the `draw` callbacks inside its own reconciler commit, which lands after this
 * component's effects. The app also boots asynchronously, so the first few
 * commits retry until the renderer exists.
 */
export function useRenderOnDemand(ref) {
  // The palette is read at draw time (`P()`), so a theme switch changes the
  // pixels without changing any prop. Subscribing here, in the hook that owns
  // the `render()` call, is what makes every Pixi panel repaint on switch — a
  // panel whose own component never re-renders (its children read the theme)
  // would otherwise keep the old theme painted on the canvas.
  useTheme();
  const [ready, setReady] = useState(false);
  useEffect(() => {
    if (ready) return undefined;
    let frame = 0;
    let tries = 0;
    const retry = () => {
      const app = ref.current?.getApplication?.();
      if (app?.renderer) { app.ticker.stop(); setReady(true); return; }
      if (++tries <= 60) frame = requestAnimationFrame(retry);
    };
    frame = requestAnimationFrame(retry);
    return () => cancelAnimationFrame(frame);
  }, [ref, ready]);
  // No dependency array: one render per commit is the whole point. The playhead
  // during playback is a React state update, so it still animates.
  useEffect(() => {
    if (!ready) return undefined;
    const app = ref.current?.getApplication?.();
    if (!app?.renderer) return undefined;
    const frame = requestAnimationFrame(() => app.render());
    return () => cancelAnimationFrame(frame);
  });
}