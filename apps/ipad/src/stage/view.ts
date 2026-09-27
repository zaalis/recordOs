/** Local zoom & pan of the remote screen (does not change what the Mac renders). */

export interface ViewState {
  scale: number;
  x: number;
  y: number;
}

export class ViewTransform {
  state: ViewState = { scale: 1, x: 0, y: 0 };
  private listeners = new Set<(state: ViewState) => void>();
  private bounds = { width: 1, height: 1 };
  readonly maxScale = 6;

  subscribe(listener: (state: ViewState) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  setBounds(width: number, height: number): void {
    this.bounds = { width: Math.max(1, width), height: Math.max(1, height) };
    this.commit(this.state);
  }

  private clamp(state: ViewState): ViewState {
    const scale = Math.min(this.maxScale, Math.max(1, state.scale));
    // Keep the scaled stage covering the viewport: translation limited to the overflow.
    const maxX = ((scale - 1) * this.bounds.width) / 2;
    const maxY = ((scale - 1) * this.bounds.height) / 2;
    return { scale, x: Math.min(maxX, Math.max(-maxX, state.x)), y: Math.min(maxY, Math.max(-maxY, state.y)) };
  }

  private commit(next: ViewState): void {
    this.state = this.clamp(next);
    for (const listener of this.listeners) listener(this.state);
  }

  /** Zooms around a client point (relative to the stage center). */
  zoomAt(cx: number, cy: number, factor: number): void {
    const { scale, x, y } = this.state;
    const nextScale = Math.min(this.maxScale, Math.max(1, scale * factor));
    const applied = nextScale / scale;
    const px = cx - this.bounds.width / 2;
    const py = cy - this.bounds.height / 2;
    this.commit({ scale: nextScale, x: px - (px - x) * applied, y: py - (py - y) * applied });
  }

  panBy(dx: number, dy: number): void {
    this.commit({ ...this.state, x: this.state.x + dx, y: this.state.y + dy });
  }

  reset(): void {
    this.commit({ scale: 1, x: 0, y: 0 });
  }

  get zoomed(): boolean {
    return this.state.scale > 1.01;
  }
}
