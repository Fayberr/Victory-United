import { readFile, writeFile } from 'node:fs/promises';
import { parseBenchmark, parsePlayLog } from '../docs/benchmark-data.js';

const width = 920;
const goal = 30;
const frameBudget = 1000 / goal;
const colours = ['#91ADFF', '#63E6C5', '#F3A4C5', '#CCADF9'];
const target = '#FFC186';
const ink = '#EDF4FF';
const panel = '#0E1829';
const muted = '#A6B7CD';

function escapeXml(value) {
  const entities = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&apos;' };
  return String(value).replace(/[&<>"']/g, char => entities[char]);
}

const compact = value => Number(value.toFixed(2));
const decimal = value => value.toFixed(1);
const runColour = index => colours[index % colours.length];

function chart(title, subtitle, description, height, elements) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" role="img" aria-labelledby="title description">
  <title id="title">${escapeXml(title)}</title>
  <desc id="description">${escapeXml(description)}</desc>
  <style>
    text { font-family: system-ui, 'Segoe UI', sans-serif; font-variant-numeric: tabular-nums; }
    .muted { fill: ${muted}; font-size: 16px; }
    .tick { fill: ${muted}; font-size: 15px; }
    .grid { stroke: #32435B; stroke-width: 1; }
    .guide { stroke: ${target}; stroke-width: 2; stroke-dasharray: 5 5; }
  </style>
  <rect x=".5" y=".5" width="${width - 1}" height="${height - 1}" rx="16" fill="${panel}" stroke="#3A4B60"/>
  <text x="34" y="52" fill="${ink}" font-size="32" font-weight="730">${escapeXml(title)}</text>
  <text x="36" y="80" class="muted">${escapeXml(subtitle)}</text>
${elements.join('\n')}
</svg>
`;
}

function scale(left, right, maximum) {
  return value => compact(left + (right - left) * value / maximum);
}

function verticalGrid(x, top, bottom, maximum, step, unit) {
  const grid = [];
  for (let value = 0; value <= maximum; value += step) {
    const position = x(value);
    grid.push(`  <path d="M${position} ${top}V${bottom}" class="grid"/>
  <text x="${position}" y="${bottom + 23}" class="tick" text-anchor="middle">${value}${value === maximum ? ' ' + unit : ''}</text>`);
  }
  return grid.join('\n');
}

function diamond(x, y, colour) {
  return `<path d="M${x} ${y - 6}l6 6-6 6-6-6z" fill="${panel}" stroke="${colour}" stroke-width="2"/>`;
}

function frameRateChart({ build, profiles }) {
  const left = 246;
  const maximum = Math.ceil(Math.max(goal + 5, ...profiles.map(p => p.fps.p90 + 5)) / 10) * 10;
  const x = scale(left, 756, maximum);
  const bottom = 146 + profiles.length * 79;
  const elements = [
    `  <path d="M36 95H884" class="grid"/>
  ${diamond(left + 6, 117, ink)}
  <text x="${left + 20}" y="122" class="tick">1% low</text>
  <path d="M${left + 105} 117h26" stroke="${muted}" stroke-width="6" stroke-linecap="round"/>
  <text x="${left + 141}" y="122" class="tick">P10–P90</text>
  <circle cx="${left + 233}" cy="117" r="6" fill="${ink}"/>
  <text x="${left + 247}" y="122" class="tick">Median</text>`,
    verticalGrid(x, 145, bottom, maximum, maximum >= 60 ? 10 : 5, 'fps'),
    `  <path d="M${x(goal)} 146V${bottom}" class="guide"/>
  <text x="${x(goal)}" y="128" fill="${target}" font-size="16" font-weight="650" text-anchor="middle">30 fps target</text>`,
  ];
  profiles.forEach(({ label, samples, fps }, index) => {
    const y = 168 + index * 79;
    const colour = runColour(index);
    const percentile = `P10–P90 ${decimal(fps.p10)}–${decimal(fps.p90)}`;
    elements.push(`  <text x="36" y="${y + 3}" fill="${ink}" font-size="20" font-weight="690">${escapeXml(label)}</text>
  <text x="36" y="${y + 28}" class="muted">${samples} samples</text>
  <path d="M${left} ${y}H${x(maximum)}" stroke="#26384F" stroke-width="8" stroke-linecap="round"/>
  <path d="M${x(fps.p10)} ${y}H${x(fps.p90)}" stroke="${colour}" stroke-width="9" stroke-linecap="round"/>
  ${diamond(x(fps.low), y, colour)}
  <circle cx="${x(fps.median)}" cy="${y}" r="7" fill="${ink}" stroke="${panel}" stroke-width="2"/>
  <text x="${left}" y="${y + 28}" class="muted">1% low ${decimal(fps.low)} · ${percentile}</text>
  <text x="882" y="${y + 8}" fill="${colour}" text-anchor="end" font-size="25" font-weight="720">${decimal(fps.median)} <tspan font-size="15">fps</tspan></text>`);
  });
  const description = profiles.map(({ label, fps }) =>
    `${label} median ${decimal(fps.median)} fps, 1% low ${decimal(fps.low)}, P10 to P90 ${decimal(fps.p10)} to ${decimal(fps.p90)}.`).join(' ');
  return chart('Frame rate by run', `RTX 4090 · Build ${build} · In-world samples`, description, bottom + 47, elements);
}

const timingCounters = [
  ['Frame time', 'frame'],
  ['Preparing', 'prepare'],
  ['Copying back', 'back'],
  ['Copies', 'copy'],
];

function frameTimingChart({ profiles }) {
  const left = 190;
  const maximum = Math.max(40, Math.ceil(Math.max(...profiles.map(p => p.timing.frame + 5),
    ...profiles.flatMap(p => timingCounters.map(([, key]) => p.timing[key]))) / 20) * 20);
  const x = scale(left, 820, maximum);
  const trackHeight = profiles.length * 21;
  const groupHeight = trackHeight + 19;
  const top = 150 + Math.floor((profiles.length - 1) / 4) * 26;
  const bottom = top + timingCounters.length * groupHeight - 14;
  const elements = [
    '  <path d="M36 95H884" class="grid"/>',
    ...profiles.map(({ label }, index) => {
      const position = 38 + index % 4 * 205;
      const legendY = 110 + Math.floor(index / 4) * 26;
      return `  <rect x="${position}" y="${legendY}" width="16" height="8" rx="4" fill="${runColour(index)}"/>
  <text x="${position + 25}" y="${legendY + 10}" class="muted">${escapeXml(label)}</text>`;
    }),
    verticalGrid(x, top, bottom, maximum, maximum / 5, 'ms'),
  ];
  timingCounters.forEach(([label, key], group) => {
    const y = top + group * groupHeight;
    elements.push(`  <text x="36" y="${y + trackHeight / 2 + 6}" fill="${ink}" font-size="18" font-weight="650">${label}</text>`);
    profiles.forEach(({ timing }, index) => {
      const barY = y + 4 + index * 21;
      const end = x(timing[key]);
      const colour = runColour(index);
      elements.push(`  <rect x="${left}" y="${barY}" width="${compact(end - left)}" height="11" rx="5.5" fill="${colour}"/>
  <text x="${end + 8}" y="${barY + 11}" fill="${colour}" font-size="15" font-weight="650">${decimal(timing[key])}</text>`);
    });
  });
  elements.push(`  <path d="M${x(frameBudget)} ${top - 3}V${top + trackHeight}" class="guide"/>
  <text x="${x(frameBudget)}" y="${top - 11}" fill="${target}" font-size="15" font-weight="650" text-anchor="middle">33.3 ms at 30 fps</text>`);
  const description = profiles.map(p =>
    p.label + ' – ' + timingCounters.map(([label, key]) => label + ' ' + decimal(p.timing[key]) + ' ms').join(', ') + '.').join(' ');
  return chart('Frame timing by run', 'Median values for four recorded counters', description, bottom + 51, elements);
}

function sessionChart({ profile, build, minutes }) {
  const left = 70;
  const right = 862;
  const top = 153;
  const bottom = 356;
  const maximum = Math.max(goal, Math.ceil(Math.max(...minutes.map(p => p.fps)) / 5) * 5);
  const first = minutes[0];
  const last = minutes.at(-1);
  const toX = scale(left, right, Math.max(1, last.elapsed - first.elapsed));
  const y = value => compact(bottom - (bottom - top) * value / maximum);
  const x = point => toX(point.elapsed - first.elapsed);
  const high = minutes.reduce((a, b) => a.fps >= b.fps ? a : b);
  const low = minutes.reduce((a, b) => a.fps <= b.fps ? a : b);
  const colour = colours.at(-1);
  const stats = [
    ['Peak minute', high], ['Lowest minute', low], ['Final minute', last],
  ];
  const elements = [
    '  <path d="M36 95H884" class="grid"/>',
    ...stats.map(([label, sample], index) => {
      const position = 36 + index * 286;
      return `  <text x="${position}" y="124" fill="${colour}" font-size="18" font-weight="700">${decimal(sample.fps)} fps</text>
  <text x="${position + 80}" y="124" class="tick">${label}</text>`;
    }),
  ];
  for (let fps = 0; fps <= maximum; fps += 5) {
    elements.push(`  <path d="M${left} ${y(fps)}H${right}" class="grid"/>
  <text x="58" y="${y(fps) + 5}" class="tick" text-anchor="end">${fps}</text>`);
  }
  const path = minutes.map((point, index) => {
    const connected = index > 0 && point.elapsed === minutes[index - 1].elapsed + 1;
    return (connected ? 'L' : 'M') + x(point) + ' ' + y(point.fps);
  }).join('');
  elements.push(`  <path d="M${left} ${y(goal)}H${right}" class="guide"/>
  <text x="${right - 5}" y="${y(goal) + 21}" fill="${target}" font-size="15" font-weight="650" text-anchor="end">30 fps target</text>
  <path d="${path}" fill="none" stroke="${colour}" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/>`);
  for (const point of minutes) {
    const annotated = point === high || point === low || point === last;
    elements.push(`  <circle cx="${x(point)}" cy="${y(point.fps)}" r="${annotated ? 5 : 2}" fill="${colour}"${annotated ? ' stroke="' + panel + '" stroke-width="1.5"' : ''}/>`);
  }
  const tickIndices = new Set([0, .25, .5, .75, 1].map(part => Math.round((minutes.length - 1) * part)));
  for (const index of tickIndices) {
    const point = minutes[index];
    elements.push(`  <text x="${x(point)}" y="382" class="tick" text-anchor="middle">${escapeXml(point.time)}</text>`);
  }
  elements.push('  <text x="862" y="409" class="tick" text-anchor="end">Time (UTC)</text>');
  const description = `${profile} from ${first.time} to ${last.time} UTC. The per-minute medians range from ${decimal(low.fps)} to ${decimal(high.fps)} fps. The final minute measured ${decimal(last.fps)} fps.`;
  return chart('Frame rate over the playtest', `${profile} · Build ${build} · FPS median for each logged minute`, description, 430, elements);
}

const measurement = parseBenchmark(await readFile(new URL('../docs/measurements.md', import.meta.url), 'utf8'));
const playLog = parsePlayLog(await readFile(new URL('../docs/play-log.md', import.meta.url), 'utf8'));
const charts = {
  'profile-frame-rate.svg': frameRateChart(measurement),
  'profile-frame-timing.svg': frameTimingChart(measurement),
  'session-frame-rate.svg': sessionChart(playLog),
};

await Promise.all(Object.entries(charts).map(([name, svg]) =>
  writeFile(new URL('../docs/charts/' + name, import.meta.url), svg)));
console.log('Updated ' + Object.keys(charts).join(', ') + '.');
