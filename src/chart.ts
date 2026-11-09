// Tiny dependency-free SVG chart: healthy-pace line vs actual usage.

export interface ChartInput {
  curve: [number, number][];
  history: [number, number][];
  nowX: number;
  cycleMinutes: number;
  /** Optional: (x, used) appended so the line reaches "now". */
  current?: number | null;
  width?: number;
  height?: number;
}

export function chartSvg(c: ChartInput): string {
  const W = c.width ?? 320;
  const H = c.height ?? 96;
  const pad = { l: 2, r: 2, t: 6, b: 4 };
  const iw = W - pad.l - pad.r;
  const ih = H - pad.t - pad.b;
  const span = Math.max(1, c.cycleMinutes);

  const pts = [...c.history];
  if (c.current != null && (pts.length === 0 || pts[pts.length - 1][0] < c.nowX)) {
    pts.push([c.nowX, c.current]);
  }
  const yMax = Math.max(100, ...pts.map((p) => p[1]));
  const X = (x: number) => pad.l + (Math.min(Math.max(x, 0), span) / span) * iw;
  const Y = (y: number) => pad.t + ih - (Math.max(0, y) / yMax) * ih;
  const f = (n: number) => n.toFixed(1);
  const line = (p: [number, number][]) => p.map(([x, y], i) => `${i ? 'L' : 'M'}${f(X(x))},${f(Y(y))}`).join('');

  const grid = [25, 50, 75, 100]
    .map((g) => `<line class="grid" x1="${pad.l}" x2="${W - pad.r}" y1="${f(Y(g))}" y2="${f(Y(g))}"/>`)
    .join('');

  const pace = c.curve.length ? `<path class="pace" d="${line(c.curve)}"/>` : '';

  let usage = '';
  if (pts.length) {
    // Step-after shape: usage holds its value until the next observation.
    const steps: [number, number][] = [];
    pts.forEach(([x, y], i) => {
      if (i > 0) steps.push([x, pts[i - 1][1]]);
      steps.push([x, y]);
    });
    const d = line(steps);
    const first = steps[0];
    const last = steps[steps.length - 1];
    const area = `${d}L${f(X(last[0]))},${f(Y(0))}L${f(X(first[0]))},${f(Y(0))}Z`;
    usage = `<path class="usage-area" d="${area}"/><path class="usage" d="${d}"/>` +
      `<circle class="usage-dot" cx="${f(X(last[0]))}" cy="${f(Y(last[1]))}" r="3"/>`;
  }

  const now = `<line class="now" x1="${f(X(c.nowX))}" x2="${f(X(c.nowX))}" y1="${pad.t}" y2="${H - pad.b}"/>`;

  return `<svg viewBox="0 0 ${W} ${H}" preserveAspectRatio="none" role="img" aria-label="Usage versus healthy pace this cycle">${grid}${now}${pace}${usage}</svg>`;
}
