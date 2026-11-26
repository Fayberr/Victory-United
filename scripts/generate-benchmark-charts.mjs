import { readFile, writeFile } from 'node:fs/promises';
import { parseBenchmark, parsePlayLog } from '../docs/benchmark-data.js';

const goal = 30;
const budget = 1000 / goal;
const palette = ['#89EED5', '#B9BCFF', '#F8AFCB', '#D3DA96'];
const target = '#FFC693';
const width = 880;
const duration = '8s';

const measurement = parseBenchmark(await readFile(new URL('../docs/measurements.md', import.meta.url), 'utf8'));
const log = parsePlayLog(await readFile(new URL('../docs/play-log.md', import.meta.url), 'utf8'));

const charts = {
  'profile-frame-rate.svg': frameRateChart(measurement),
  'profile-frame-timing.svg': timingChart(measurement),
  'session-frame-rate.svg': sessionChart(log),
};

await Promise.all(Object.entries(charts).map(([name, svg]) =>
  writeFile(new URL('../docs/charts/' + name, import.meta.url), svg)));
console.log('Updated ' + Object.keys(charts).join(', ') + '.');

function escapeXml(value) {
  return String(value).replace(/[&<>"']/g, char => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&apos;',
  })[char]);
}

function color(index) {
  return palette[index % palette.length];
}

function px(value) {
  return Number(value.toFixed(2));
}

function decimal(value) {
  return value.toFixed(1);
}

function svgChart(title, subtitle, description, height, plot, extraDefs = '') {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" role="img" aria-labelledby="title description">
  <title id="title">${escapeXml(title)}</title>
  <desc id="description">${escapeXml(description)}</desc>
  <defs>
    <linearGradient id="surface" x2="1" y2="1">
      <stop stop-color="#252939"/>
      <stop offset="1" stop-color="#151824"/>
    </linearGradient>
${extraDefs}
  </defs>
  <style>
    text { font-family: system-ui, 'Segoe UI', sans-serif; font-variant-numeric: tabular-nums; }
    .subtitle { fill: #B7BED3; font-size: 18px; }
    .tick { fill: #B7BED3; font-size: 16px; }
    .annotation { paint-order: stroke; stroke: #141825; stroke-width: 4px; }
    .axis-line { stroke: #3E465D; stroke-dasharray: 2 5; }
  </style>
  <rect x=".5" y=".5" width="${width - 1}" height="${height - 1}" rx="20" fill="url(#surface)" stroke="#424960"/>
  <text x="34" y="52" fill="#F5F7FC" font-size="33" font-weight="730">${escapeXml(title)}</text>
  <text x="36" y="80" class="subtitle">${escapeXml(subtitle)}</text>
${plot}
</svg>
`;
}

function grid(x, y, size, bottom, maximum, tickCount, unit) {
  const marks = [];
  for (let tick = 0; tick <= tickCount; tick++) {
    const value = maximum * tick / tickCount;
    const pos = px(x + size * value / maximum);
    marks.push(`  <path d="M${pos} ${y}V${bottom}" class="axis-line"/>
  <text x="${pos}" y="${bottom + 22}" class="tick" text-anchor="middle">${value}${value === maximum ? ' ' + unit : ''}</text>`);
  }
  return marks.join('\n');
}

function reveal(attribute, from, to, start, end) {
  return `<animate attributeName="${attribute}" values="${from};${from};${to};${to};${from};${from}"
      keyTimes="0;${start};${end};.95;.985;1" calcMode="spline"
      keySplines="0 0 1 1;.32 .12 .26 1;0 0 1 1;0 0 1 1;0 0 1 1"
      dur="${duration}" repeatCount="indefinite"/>`;
}

function motionGroup(contents) {
  return `  <g>
    <animate attributeName="opacity" values="1;1;0;0;1" keyTimes="0;.86;.95;.985;1" dur="${duration}" repeatCount="indefinite"/>
${contents}
  </g>`;
}

function animatedBar(x, y, size, height, fill, start, end) {
  const radius = Math.min(height / 2, 7);
  return `    <rect x="${x}" y="${y}" width="${size}" height="${height}" rx="${radius}" fill="${fill}">
      ${reveal('width', 0, size, start, end)}
    </rect>`;
}

function frameRateChart({ build, profiles }) {
  const plotX = 222;
  const plotWidth = 596;
  const maximum = Math.max(goal, Math.ceil(Math.max(...profiles.map(p => p.fps.p90)) / 10) * 10);
  const x = value => px(plotX + plotWidth * value / maximum);
  const bottom = 145 + (profiles.length - 1) * 91 + 56;
  const ghost = [];
  const animated = [];
  const marks = [];
  profiles.forEach(({ name, fps, samples }, index) => {
    const y = 145 + index * 91;
    const hue = color(index);
    const median = x(fps.median);
    const low = x(fps.low);
    const left = x(fps.p10);
    const right = x(fps.p90);
    const rangeLabel = Math.min(743, Math.max(306, px((left + right) / 2)));
    const lowLabel = Math.min(758, Math.max(262, low));
    const start = px(.04 + .18 * index / profiles.length);
    const end = px(start + .24);
    ghost.push(`  <text x="51" y="${y - 11}" fill="#EEF1FA" font-size="19" font-weight="660">${escapeXml(name)}</text>
  <text x="51" y="${y + 18}" fill="${hue}" font-size="27" font-weight="730">${decimal(fps.median)} <tspan font-size="17">fps</tspan></text>
  <text x="51" y="${y + 40}" class="tick">${samples} samples</text>
  <rect x="${plotX}" y="${y - 7}" width="${plotWidth}" height="14" rx="7" fill="#2F3548"/>
  <rect x="${plotX}" y="${y - 7}" width="${px(median - plotX)}" height="14" rx="7" fill="${hue}" opacity=".25"/>`);
    animated.push(animatedBar(plotX, y - 7, px(median - plotX), 14, hue, start, end));
    marks.push(`  <path d="M${low} ${y - 7}l7 7-7 7-7-7z" fill="#D9DFFC" stroke="#141825" stroke-width="2"/>
  <text x="${lowLabel}" y="${y - 17}" class="tick annotation" text-anchor="middle">1% low ${decimal(fps.low)}</text>
  <circle cx="${median}" cy="${y}" r="7" fill="${hue}" stroke="#141825" stroke-width="2"/>
  <path d="M${left} ${y + 12}v7H${right}v-7" fill="none" stroke="${hue}" stroke-width="2"/>
  <text x="${rangeLabel}" y="${y + 40}" fill="${hue}" font-size="17" font-weight="610" text-anchor="middle">P10–P90 ${decimal(fps.p10)}–${decimal(fps.p90)}</text>
  <circle cx="${median}" cy="${y}" r="9" fill="none" stroke="${hue}" opacity="0">
    <animate attributeName="r" values="9;9;20;20" keyTimes="0;${end};${px(end + .16)};1" dur="${duration}" repeatCount="indefinite"/>
    <animate attributeName="opacity" values="0;0;.8;0;0" keyTimes="0;${px(end - .02)};${px(end + .03)};${px(end + .16)};1" dur="${duration}" repeatCount="indefinite"/>
  </circle>`);
  });
  const description = profiles.map(p => `${p.name} median ${decimal(p.fps.median)} fps, 1% low ${decimal(p.fps.low)}, p10 to p90 ${decimal(p.fps.p10)} to ${decimal(p.fps.p90)}.`).join(' ');
  const plot = [
    `  <rect x="30" y="98" width="820" height="${bottom - 58}" rx="12" fill="#141825" stroke="#343C50"/>`,
    grid(plotX, 135, plotWidth, bottom, maximum, maximum === goal ? 6 : 4, 'fps'),
    ...ghost, motionGroup(animated.join('\n')), ...marks,
    `  <path d="M${x(goal)} 125V${bottom}" stroke="${target}" stroke-width="2" stroke-dasharray="4 5"/>
  <text x="${x(goal) - 9}" y="121" fill="${target}" font-size="17" font-weight="680" text-anchor="end">30 fps target</text>`,
  ].join('\n');
  return svgChart('Frame rate in the world', 'RTX 4090 · Build ' + build + ' · median FPS by profile',
    description + ' The target is 30 fps.', bottom + 54, plot);
}

function timingChart({ profiles }) {
  const counters = [
    ['Frame time', 'frame'],
    ['Preparing', 'prepare'],
    ['Copying back', 'back'],
    ['Copies', 'copy'],
  ];
  const plotX = 206;
  const plotWidth = 557;
  const maximum = Math.max(40, Math.ceil(Math.max(...profiles.flatMap(p => counters.map(([, key]) => p.timing[key]))) / 20) * 20);
  const sectionHeight = profiles.length * 24 + 20;
  const bottom = 138 + counters.length * sectionHeight - 8;
  const x = value => px(plotX + plotWidth * value / maximum);
  const legends = profiles.map(({ name }, i) => {
    const left = 842 - (profiles.length - i) * 96;
    return `  <circle cx="${left}" cy="47" r="5" fill="${color(i)}"/><text x="${left + 12}" y="53" fill="${color(i)}" font-size="16" font-weight="640">${escapeXml(name)}</text>`;
  });
  const ghost = [];
  const animated = [];
  counters.forEach(([label, key], group) => {
    const top = 137 + group * sectionHeight;
    const center = top + (profiles.length - 1) * 12 + 12;
    ghost.push(`  <text x="49" y="${center}" fill="#F2F3FA" font-size="19" font-weight="650">${label}</text>`);
    profiles.forEach(({ timing }, index) => {
      const hue = color(index);
      const y = top + index * 24;
      const value = timing[key];
      const size = px(x(value) - plotX);
      const start = px(.04 + group * .065 + index * .09 / profiles.length);
      ghost.push(`  <rect x="${plotX}" y="${y}" width="${plotWidth}" height="14" rx="7" fill="#2F3548"/>
  <rect x="${plotX}" y="${y}" width="${size}" height="14" rx="7" fill="${hue}" opacity=".25"/>
  <text x="828" y="${y + 13}" fill="${hue}" font-size="17" font-weight="680" text-anchor="end">${decimal(value)} ms</text>`);
      animated.push(animatedBar(plotX, y, size, 14, hue, start, px(start + .21)));
    });
  });
  const plot = [
    ...legends,
    `  <rect x="30" y="98" width="820" height="${bottom - 58}" rx="12" fill="#141825" stroke="#343C50"/>`,
    grid(plotX, 122, plotWidth, bottom, maximum, 4, 'ms'),
    ...ghost, motionGroup(animated.join('\n')),
    `  <path d="M${x(budget)} 127v${profiles.length * 24 + 4}" stroke="#171B27" stroke-width="5"/>
  <path d="M${x(budget)} 127v${profiles.length * 24 + 4}" stroke="${target}" stroke-width="2"/>
  <text x="${x(budget)}" y="119" fill="${target}" font-size="16" font-weight="660" text-anchor="middle">33.3 ms budget at 30 fps</text>`,
  ].join('\n');
  const description = profiles.map(p => p.name + ' ' + counters.map(([label, key]) =>
    label.toLowerCase() + ' ' + decimal(p.timing[key]) + ' ms').join(', ') + '.').join(' ');
  return svgChart('Time per frame', 'Each timing counter is a separate median', description, bottom + 54, plot);
}

function sessionChart({ profile, build, minutes }) {
  const left = 68;
  const right = 824;
  const top = 129;
  const bottom = 309;
  const maximum = Math.max(goal, Math.ceil(Math.max(...minutes.map(p => p.fps)) / 5) * 5);
  const elapsed = Math.max(1, minutes.at(-1).elapsed - minutes[0].elapsed);
  const height = 386;
  const y = value => px(bottom - (bottom - top) * value / maximum);
  const points = minutes.map(p => ({ ...p,
    x: px(left + (right - left) * (p.elapsed - minutes[0].elapsed) / elapsed), y: y(p.fps),
  }));
  const high = points.reduce((a, b) => a.fps >= b.fps ? a : b);
  const low = points.reduce((a, b) => a.fps <= b.fps ? a : b);
  const last = points.at(-1);
  const segments = [];
  for (const point of points) {
    if (!segments.length || point.elapsed > segments.at(-1).at(-1).elapsed + 1) segments.push([]);
    segments.at(-1).push(point);
  }
  const paths = segments.filter(points => points.length > 1).map(points => ({
    start: points[0].x, line: 'M' + points.map(p => p.x + ' ' + p.y).join('L'),
  }));
  const gridLines = [];
  const step = maximum === goal ? 5 : maximum / 5;
  for (let fps = 0; fps <= maximum; fps += step) {
    gridLines.push(`  <path d="M${left} ${y(fps)}H${right}" class="axis-line"/><text x="55" y="${y(fps) + 5}" class="tick" text-anchor="end">${fps}</text>`);
  }
  const ticks = [...new Set([0, .25, .5, .75, 1].map(fraction => Math.round((points.length - 1) * fraction)))];
  const labels = ticks.map(index => {
    const point = points[index];
    return `  <text x="${point.x}" y="338" class="tick" text-anchor="middle">${point.time}</text>`;
  });
  const extremes = high === low ? [] : [[high, 'high', -16], [low, 'low', 25]].map(([point, name, offset]) =>
    `  <text x="${Math.min(757, Math.max(122, point.x))}" y="${point.y + offset}" fill="#D2D1FF" font-size="17" font-weight="690" text-anchor="middle" paint-order="stroke" stroke="#141825" stroke-width="4">${decimal(point.fps)} fps ${name}</text>`);
  const plot = [
    `  <rect x="30" y="99" width="820" height="264" rx="12" fill="#141825" stroke="#343C50"/>
  <text x="840" y="51" fill="#D2D1FF" font-size="26" font-weight="730" text-anchor="end">${decimal(last.fps)} <tspan font-size="17">fps</tspan></text>
  <text x="840" y="79" class="tick" text-anchor="end">${last.time} UTC · latest minute</text>`,
    ...gridLines,
    `  <path d="M${left} ${y(goal)}H${right}" stroke="${target}" stroke-width="2" stroke-dasharray="5 6"/>
  <text x="${right}" y="${y(goal) - 9}" fill="${target}" font-size="16" font-weight="650" text-anchor="end">30 fps target</text>`,
    ...paths.map(({ start, line }) => `  <path d="${line}V${bottom}H${start}Z" fill="url(#area)"/>
  <path d="${line}" fill="none" stroke="#7679A7" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/>`),
    motionGroup(paths.map(({ line }) => `    <path d="${line}" fill="none" stroke="#B9BCFF" stroke-width="4" stroke-linecap="round" stroke-linejoin="round" pathLength="1" stroke-dasharray="1">
      ${reveal('stroke-dashoffset', 1, 0, .05, .53)}
    </path>`).join('\n')),
    ...points.map(p => `  <circle cx="${p.x}" cy="${p.y}" r="${p === last ? 6 : 3}" fill="#C3C7FF"/>`),
    ...extremes,
    ...labels,
    `  <text x="824" y="357" class="tick" text-anchor="end">UTC</text>`,
  ].join('\n');
  return svgChart('Frame rate over the session', profile + ' · per-minute medians · Build ' + build,
    `${profile}, ${points.length} logged minutes from ${points[0].time} to ${last.time} UTC. The minute medians range from ${decimal(low.fps)} to ${decimal(high.fps)} fps. The latest minute was ${decimal(last.fps)} fps.`,
    height, plot, `    <linearGradient id="area" x2="0" y2="1"><stop stop-color="#9D9DF1" stop-opacity=".33"/><stop offset="1" stop-color="#9D9DF1" stop-opacity=".015"/></linearGradient>`);
}
