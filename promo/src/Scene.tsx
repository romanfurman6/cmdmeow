import React, {useLayoutEffect, useRef, useState} from 'react';
import {continueRender, delayRender, staticFile} from 'remotion';

export type SceneModule = {
  css: string;
  body: string;
  create: (root: HTMLElement) => {render: (t: number) => void; init: () => void};
};

// Mounts one scene (1920x1080 stage) and draws it for time t (seconds).
export const Scene: React.FC<{mod: SceneModule; t: number; onDrawn?: (root: HTMLElement) => void}> = ({mod, t, onDrawn}) => {
  const ref = useRef<HTMLDivElement>(null);
  const inst = useRef<ReturnType<SceneModule['create']> | null>(null);
  const [handle] = useState(() => delayRender('scene fonts + layout'));
  const [ready, setReady] = useState(false);
  const css = mod.css.replace(/url\((desk[\w-]*\.jpg)\)/g, (_m, f) => `url(${staticFile(f)})`);

  useLayoutEffect(() => {
    const root = ref.current!;
    root.innerHTML = mod.body;
    inst.current = mod.create(root);
    let live = true;
    document.fonts.ready.then(() => {
      if (!live) return;
      inst.current!.init();
      setReady(true);
      continueRender(handle);
    });
    return () => {
      live = false;
    };
  }, [mod, handle]);

  useLayoutEffect(() => {
    if (!ready || !inst.current) return;
    inst.current.render(t);
    if (onDrawn) onDrawn(ref.current!);
  }, [t, ready, onDrawn]);

  return (
    <>
      <style>{css}</style>
      <div ref={ref} style={{position: 'absolute', left: 0, top: 0, width: 1920, height: 1080, overflow: 'hidden'}} />
    </>
  );
};
