import { useEffect, useRef, type RefObject } from "react";
import type { SurfaceStyle } from "../lib/storage.ts";

/** Local visual feedback only. Its bounded canvas never sends input or changes the session. */
export function PointerTrail({
  target,
  style,
}: {
  target: RefObject<HTMLDivElement | null>;
  style: SurfaceStyle;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const surface = target.current,
      element = canvas.current;
    if (
      !style.trail ||
      !surface ||
      !element ||
      matchMedia("(prefers-reduced-motion: reduce)").matches
    )
      return;
    const ctx = element.getContext("2d");
    if (!ctx) return;
    let points: { x: number; y: number; t: number; id: number }[] = [],
      frame = 0;
    const resize = () => {
      const ratio = Math.min(2, devicePixelRatio || 1);
      element.width = Math.round(innerWidth * ratio);
      element.height = Math.round(innerHeight * ratio);
      ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
      points = [];
    };
    const draw = () => {
      frame = 0;
      const now = performance.now();
      points = points.filter((p) => now - p.t < style.trailDuration);
      ctx.clearRect(0, 0, innerWidth, innerHeight);
      ctx.strokeStyle = style.trailColor;
      ctx.lineCap = "round";
      ctx.lineWidth = style.trailSize;
      ctx.shadowColor = style.trailColor;
      ctx.shadowBlur = style.trailSize;
      for (let i = 1; i < points.length; i++) {
        const a = points[i - 1]!,
          b = points[i]!;
        if (a.id !== b.id) continue;
        ctx.globalAlpha =
          Math.max(0, 1 - (now - b.t) / style.trailDuration) * 0.28;
        ctx.beginPath();
        ctx.moveTo(a.x, a.y);
        ctx.lineTo(b.x, b.y);
        ctx.stroke();
      }
      if (points.length) frame = requestAnimationFrame(draw);
    };
    const held = new Set<number>();
    let owner = "",
      stroke = 0;
    const down = (e: PointerEvent) => {
      if (owner && owner !== e.pointerType) return;
      owner = e.pointerType;
      held.add(e.pointerId);
      stroke++;
    };
    const up = (e: PointerEvent) => {
      held.delete(e.pointerId);
      if (!held.size) {
        owner = "";
        stroke++;
      }
    };
    const move = (e: PointerEvent) => {
      if (owner && owner !== e.pointerType) return;
      points.push({
        x: e.clientX,
        y: e.clientY,
        t: performance.now(),
        id: e.pointerId + stroke * 100000,
      });
      if (points.length > 96) points.shift();
      if (!frame) frame = requestAnimationFrame(draw);
    };
    resize();
    window.addEventListener("resize", resize);
    surface.addEventListener("pointerdown", down);
    surface.addEventListener("pointerup", up);
    surface.addEventListener("pointercancel", up);
    surface.addEventListener("pointermove", move, { passive: true });
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("resize", resize);
      surface.removeEventListener("pointerdown", down);
      surface.removeEventListener("pointerup", up);
      surface.removeEventListener("pointercancel", up);
      surface.removeEventListener("pointermove", move);
      ctx.clearRect(0, 0, innerWidth, innerHeight);
    };
  }, [target, style]);
  return <canvas ref={canvas} className="pointer-trail" aria-hidden="true" />;
}
