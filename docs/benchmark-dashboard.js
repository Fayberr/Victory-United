import { parseBenchmark, parsePlayLog } from './benchmark-data.js?v=playtest';

const byId = id => document.getElementById(id);
const ui = {
  dashboard: byId('dashboard'),
  plot: byId('plot'),
  tabs: [...document.querySelectorAll('[data-chart]')],
  presets: [...document.querySelectorAll('[data-target]')],
  modes: byId('timing-modes'),
  panel: byId('chart-panel'),
  slider: byId('fps-target'),
  profile: byId('profile'),
  replay: byId('replay'),
};
const view = {
  chart: 'fps', mode: 'time', target: 30, benchmark: null, session: null,
  profile: null, selected: '', limit: 60,
};
const readings = new Map();
const formatters = [0, 1].map(digits => new Intl.NumberFormat('en', {
  minimumFractionDigits: digits,
  maximumFractionDigits: digits,
}));
const motion = matchMedia('(prefers-reduced-motion: reduce)');

function element(tag, classes, text) {
  const node = document.createElement(tag);
  if (classes) node.className = classes;
  if (text !== undefined) node.textContent = text;
  return node;
}

function number(value, digits = 1) {
  return formatters[digits].format(value);
}

function showValue(id, value, unit, digits = 1) {
  const node = byId(id);
  if (value === null) {
    node.textContent = '—';
    return;
  }
  node.replaceChildren(number(value, digits), element('span', 'unit', unit));
}

function axis(limit, unit) {
  const result = element('div', 'axis');
  result.setAttribute('aria-hidden', 'true');
  for (let i = 0; i <= 4; i++) {
    const value = number(limit * i / 4, 0) + (i === 4 ? ' ' + unit : '');
    result.append(element('span', '', value));
  }
  return result;
}

function position(value, limit = view.limit) {
  return 100 * value / limit + '%';
}

function button(classes, text, key, label = text) {
  const node = element('button', classes, text);
  node.type = 'button';
  node.dataset.inspect = key;
  node.setAttribute('aria-label', label);
  node.setAttribute('aria-pressed', 'false');
  return node;
}

function inspect(key) {
  const reading = readings.get(key)?.();
  if (!reading) return;
  view.selected = key;
  for (const control of ui.plot.querySelectorAll('[data-inspect]')) {
    control.setAttribute('aria-pressed', String(control.dataset.inspect === key));
  }
  byId('reading-title').textContent = reading.title;
  byId('reading-value').textContent = reading.value;
  byId('reading-description').textContent = reading.description;
}

function mark(key, value, title, delay) {
  const node = button('pin pin-' + key, '', key, title);
  node.dataset.reveal = 'point';
  node.dataset.delay = delay;
  node.style.setProperty('--x', position(value));
  node.append(element('span', 'pin-core'));
  return node;
}

function makeFrameRate() {
  const { median, p10, p90, low } = view.profile.fps;
  view.limit = Math.max(60, Math.ceil(p90 / 20) * 20);
  const fps = element('section', 'fps-plot');
  fps.setAttribute('aria-label', 'In-world frame rate scale');
  const track = element('div', 'plot-track');
  const meter = element('div', 'meter-fill');
  const gap = element('div', 'gap-line');
  const band = element('div', 'p-band');
  meter.dataset.reveal = 'bar';
  meter.style.setProperty('--end', position(median));
  gap.dataset.reveal = 'bar';
  gap.dataset.delay = 510;
  band.dataset.reveal = 'bar';
  band.dataset.delay = 470;
  band.style.setProperty('--start', position(p10));
  band.style.setProperty('--width', position(p90 - p10));
  track.append(
    element('div', 'plot-grid'), element('div', 'rail'), meter, gap, band,
    mark('goal', view.target, 'FPS target', 650),
    mark('low', low, '1% low ' + number(low) + ' fps', 170),
    mark('median', median, 'Median ' + number(median) + ' fps', 370),
    mark('band', (p10 + p90) / 2, '10th to 90th percentile', 560),
  );
  fps.append(track, axis(view.limit, 'fps'));

  readings.set('median', () => ({
    title: 'Median', value: number(median) + ' fps',
    description: 'The midpoint of the in-world frame-rate samples.',
  }));
  readings.set('low', () => ({
    title: '1% low', value: number(low) + ' fps',
    description: 'The reported 1% low indicates performance during the slowest part of the run.',
  }));
  readings.set('band', () => ({
    title: '10th–90th percentile', value: number(p10) + '–' + number(p90) + ' fps',
    description: 'The central 80% of the reported frame-rate distribution.',
  }));
  readings.set('goal', () => {
    const difference = Math.abs(view.target - median);
    const relation = median < view.target ? 'below' : 'above';
    return {
      title: 'FPS target', value: number(view.target, 0) + ' fps',
      description: difference === 0 ? 'The measured median matches this target.' :
        'The measured median is ' + number(difference) + ' fps ' + relation + ' this target.',
    };
  });
  const legend = element('div', 'legend');
  for (const [key, label] of [['median', 'Median'], ['low', '1% low'], ['band', '10th–90th percentile'], ['goal', 'FPS target']]) {
    const control = button('', label, key);
    const swatch = element('span', 'swatch swatch-' + key);
    swatch.setAttribute('aria-hidden', 'true');
    control.prepend(swatch);
    legend.append(control);
  }
  fps.append(legend);
  return fps;
}

function makeBars(rows, unit, limit = view.limit) {
  const plot = element('div', 'bar-plot');
  rows.forEach(([key, title, value, description, options = {}], index) => {
    const label = number(value) + ' ' + unit;
    const row = button('bar-row', '', key, title + ', ' + label);
    const rail = element('span', 'bar-rail');
    const accent = options.accent ?? (key === 'frame');
    const fill = element('span', accent ? 'bar-fill bar-frame' : 'bar-fill');
    fill.dataset.reveal = 'bar';
    fill.dataset.delay = index * 90;
    fill.style.setProperty('--end', position(value, limit));
    rail.append(fill);
    const reference = options.reference ?? (key === 'frame' ? 'budget' : null);
    if (reference) {
      const budget = element('span', 'budget-mark');
      budget.dataset.reveal = 'point';
      budget.dataset.delay = 270;
      budget.dataset.reference = reference;
      budget.dataset.limit = limit;
      budget.setAttribute('aria-hidden', 'true');
      rail.append(budget);
    }
    row.append(element('span', 'bar-label', title), rail, element('strong', 'bar-value', label));
    plot.append(row);
    readings.set(key, () => ({ title, value: label, description }));
  });
  const ruler = element('div', 'bar-axis');
  ruler.append(axis(limit, unit));
  plot.append(ruler);
  return plot;
}

function makeComparison() {
  const profiles = view.benchmark.profiles;
  const plot = element('div', 'compare-plot');
  const charts = [
    {
      title: 'Median frame rate', unit: 'fps', reference: 'fps',
      value: profile => profile.fps.median,
      minimum: Number(ui.slider.max), description: 'The gold marker is the selected FPS target.',
    },
    {
      title: 'Median frame time', unit: 'ms', reference: 'budget',
      value: profile => profile.timing.frame,
      minimum: 1000 / Number(ui.slider.min), description: 'The gold marker is the selected frame budget.',
    },
  ];
  for (const { title, unit, value, reference, minimum, description } of charts) {
    const group = element('section');
    const values = profiles.map(value);
    const limit = Math.ceil(Math.max(minimum, ...values) / 20) * 20;
    const rows = profiles.map(({ label }, index) => [
      reference + '-' + index, label, values[index], title + '. ' + description,
      { reference, accent: index === 0 },
    ]);
    group.append(element('h3', '', title), makeBars(rows, unit, limit));
    plot.append(group);
  }
  return plot;
}

function makeFrameTiming() {
  const { timing, transfers } = view.profile;
  if (view.mode === 'volume') {
    view.limit = Math.max(20, Math.ceil(Math.max(...Object.values(transfers)) / 20) * 20);
    return makeBars([
      ['copy', 'Copies', transfers.copy, 'Median data moved by the copy counter in one frame.'],
      ['texture', 'Textures', transfers.texture, 'Median texture data moved in one frame.'],
      ['back', 'Copying back', transfers.back, 'Median data copied back in one frame.'],
    ], 'MB');
  }
  view.limit = Math.max(100, Math.ceil(Math.max(...Object.values(timing)) / 20) * 20);
  const gpuReading = view.profile.name.split(',', 1)[0] === 'Series S'
    ? 'The reported Series S GPU counter is still under review.'
    : 'Median time reported by the GPU counter.';
  return makeBars([
    ['frame', 'Frame time', timing.frame, 'The median full-frame duration. The gold line marks the selected frame budget.'],
    ['gpu', 'GPU counter', timing.gpu, gpuReading],
    ['prepare', 'Preparing', timing.prepare, 'Median time reported by the preparation counter.'],
    ['back', 'Copying back', timing.back, 'Median time reported by the copy-back counter.'],
    ['copy', 'Copies', timing.copy, 'Median time reported by the copy counter.'],
    ['texture', 'Textures', timing.texture, 'Median time reported by the texture counter.'],
  ], 'ms');
}

function svgElement(tag, attrs) {
  const node = document.createElementNS('http://www.w3.org/2000/svg', tag);
  for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, value);
  return node;
}

function makeSession() {
  const { minutes } = view.session;
  const plot = element('div', 'session-plot');
  const svg = svgElement('svg', { viewBox: '0 0 780 244', 'aria-hidden': 'true' });
  const left = 50;
  const right = 744;
  const top = 26;
  const bottom = 202;
  const maximum = Math.max(30, Math.ceil(Math.max(...minutes.map(p => p.fps)) / 5) * 5);
  const elapsed = Math.max(1, minutes.at(-1).elapsed - minutes[0].elapsed);
  const y = value => bottom - (bottom - top) * value / maximum;
  const points = minutes.map(p => ({
    x: left + (right - left) * (p.elapsed - minutes[0].elapsed) / elapsed, y: y(p.fps),
  }));
  const tick = (x, y, value, anchor) => {
    const label = svgElement('text', { x, y, 'text-anchor': anchor });
    label.textContent = value;
    svg.append(label);
  };
  const step = maximum === 30 ? 5 : maximum / 5;
  for (let fps = 0; fps <= maximum; fps += step) {
    svg.append(svgElement('path', { d: 'M' + left + ' ' + y(fps) + 'H' + right, class: 'session-rule' }));
    tick(left - 8, y(fps) + 5, fps, 'end');
  }
  svg.append(svgElement('path', { d: 'M' + left + ' ' + y(30) + 'H' + right, class: 'session-target' }));
  tick(right, y(30) - 8, '30 fps target', 'end');
  const path = points.map((p, index) => {
    const connected = index > 0 && minutes[index].elapsed - minutes[index - 1].elapsed === 1;
    return (connected ? 'L' : 'M') + p.x + ' ' + p.y;
  }).join('');
  const line = svgElement('path', {
    d: path, class: 'session-reveal', pathLength: 1, 'stroke-dasharray': 1, 'stroke-dashoffset': 0,
  });
  line.dataset.reveal = 'line';
  svg.append(svgElement('path', { d: path, class: 'session-line' }), line);
  for (const index of new Set([0, .25, .5, .75, 1].map(n => Math.round(n * (minutes.length - 1))))) {
    tick(points[index].x, bottom + 25, minutes[index].time, 'middle');
  }
  const guide = svgElement('path', { class: 'session-guide' });
  const marker = svgElement('circle', { r: 6, class: 'session-point' });
  svg.append(guide, marker);
  const input = element('input');
  input.type = 'range';
  input.id = 'logged-minute';
  input.min = 0;
  input.max = minutes.length - 1;
  const caption = element('label', '', 'Logged minute');
  caption.htmlFor = input.id;
  const output = element('output');
  output.setAttribute('for', input.id);
  const controls = element('div', 'session-controls');
  controls.append(caption, input, output);
  plot.append(svg, controls);
  minutes.forEach((p, index) => {
    readings.set('minute-' + index, () => ({
      title: p.time + ' UTC',
      value: number(p.fps) + ' fps',
      description: [p.scene, 'Preparing ' + number(p.preparing) + ' ms',
        'Copying back ' + number(p.copyingBack) + ' ms'].filter(Boolean).join(' · '),
    }));
  });
  function selectMinute(index, announce = true) {
    const point = points[index];
    guide.setAttribute('d', 'M' + point.x + ' ' + top + 'V' + bottom);
    marker.setAttribute('cx', point.x);
    marker.setAttribute('cy', point.y);
    input.value = index;
    output.value = minutes[index].time + ' UTC';
    input.setAttribute('aria-valuetext', output.value);
    if (announce && view.selected !== 'minute-' + index) inspect('minute-' + index);
  }
  const moveScrubber = event => {
    const box = svg.getBoundingClientRect();
    const position = (event.clientX - box.left) / box.width * 780;
    const closest = points.reduce((best, point, index) =>
      Math.abs(point.x - position) < Math.abs(points[best].x - position) ? index : best, 0);
    selectMinute(closest);
  };
  svg.addEventListener('pointermove', event => {
    if (event.pointerType === 'mouse') moveScrubber(event);
  });
  svg.addEventListener('click', moveScrubber);
  input.addEventListener('input', () => selectMinute(input.valueAsNumber));
  selectMinute(minutes.length - 1, false);
  return plot;
}

function gpuRing(load) {
  const figure = element('figure', 'ring-reading');
  const svg = svgElement('svg', { class: 'ring', viewBox: '0 0 110 110', 'aria-hidden': 'true' });
  const circle = { cx: 55, cy: 55, r: 46, pathLength: 100 };
  const value = svgElement('circle', {
    ...circle, class: 'ring-value', 'stroke-dasharray': 100, 'stroke-dashoffset': 100 - load,
  });
  value.dataset.reveal = 'ring';
  svg.append(svgElement('circle', { ...circle, class: 'ring-track' }), value);
  const caption = element('figcaption');
  caption.append(element('strong', '', number(load, 0) + '%'), 'GPU load');
  figure.append(svg, caption);
  return figure;
}

function makeHardware() {
  const resource = view.profile.resources;
  if (!resource) {
    return element('p', 'muted', 'GPU and memory readings are pending for this profile.');
  }
  const content = element('div');
  const overview = element('div', 'hardware-overview');
  const stats = element('dl', 'hardware-stats');
  for (const [label, value] of [
    ['Median power', number(resource.power) + ' W'],
    ['Max temperature', number(resource.temperature, 0) + ' °C'],
    ['Median clock', number(resource.clock, 0) + ' MHz'],
  ]) {
    const stat = element('div');
    stat.append(element('dt', '', label), element('dd', '', value));
    stats.append(stat);
  }
  overview.append(gpuRing(resource.load), stats);
  view.limit = Math.max(20, Math.ceil(Math.max(resource.ramMax, resource.workingSet, resource.vram) / 20) * 20);
  content.append(overview, makeBars([
    ['ram-median', 'Game RAM median', resource.ramMedian, 'Median game memory across the full session.'],
    ['ram-max', 'Game RAM max', resource.ramMax, 'Highest sampled game memory during the session.'],
    ['working-set', 'Working set max', resource.workingSet, 'Highest sampled resident working set.'],
    ['vram', 'GPU memory max', resource.vram, 'Highest sampled GPU memory usage.'],
  ], 'GB'));
  return content;
}

function setDifference(labelId, valueId, label, difference, unit) {
  byId(labelId).textContent = label;
  const node = byId(valueId);
  node.textContent = number(Math.abs(difference)) + ' ' + unit;
  node.dataset.tone = difference > 0 ? 'warm' : 'cool';
}

function updateTarget(value) {
  value = Math.min(Number(ui.slider.max), Math.max(Number(ui.slider.min), value));
  view.target = value;
  ui.slider.value = value;
  ui.slider.setAttribute('aria-valuetext', value + ' frames per second');
  showValue('target-value', value, 'fps', 0);
  const frameBudget = 1000 / value;
  const fpsDelta = value - view.profile.fps.median;
  const frameDelta = view.profile.timing.frame - frameBudget;
  byId('budget-value').textContent = number(frameBudget) + ' ms';
  setDifference('fps-difference-label', 'fps-difference', fpsDelta >= 0 ? 'FPS to target' : 'FPS above target', fpsDelta, 'fps');
  setDifference('time-difference-label', 'time-difference', frameDelta >= 0 ? 'Over frame budget' : 'Under frame budget', frameDelta, 'ms');
  for (const preset of ui.presets) {
    preset.setAttribute('aria-pressed', String(Number(preset.dataset.target) === value));
  }
  const comparing = view.chart === 'compare';
  byId('fps-difference-label').parentElement.hidden = comparing;
  byId('time-difference-label').parentElement.hidden = comparing;
  const gaps = byId('comparison-gaps');
  gaps.hidden = !comparing;
  if (comparing) {
    gaps.replaceChildren(...view.benchmark.profiles.map(profile => {
      const difference = value - profile.fps.median;
      const row = element('div');
      const label = profile.label + (difference >= 0 ? ' to target' : ' above target');
      const gap = element('dd', '', number(Math.abs(difference)) + ' fps');
      gap.dataset.tone = difference > 0 ? 'warm' : 'cool';
      row.append(element('dt', '', label), gap);
      return row;
    }));
  }
  const goal = ui.plot.querySelector('.pin-goal');
  if (goal) {
    goal.style.setProperty('--x', position(value));
    goal.setAttribute('aria-label', 'FPS target ' + value + ' fps');
    goal.setAttribute('aria-keyshortcuts', 'ArrowLeft ArrowRight');
    const gap = ui.plot.querySelector('.gap-line');
    gap.style.setProperty('--start', position(Math.min(value, view.profile.fps.median)));
    gap.style.setProperty('--width', position(Math.abs(fpsDelta)));
    gap.classList.toggle('ahead', fpsDelta < 0);
  }
  for (const mark of ui.plot.querySelectorAll('.budget-mark')) {
    const reference = mark.dataset.reference === 'fps' ? value : frameBudget;
    mark.style.setProperty('--x', position(reference, Number(mark.dataset.limit)));
  }
  if (view.selected === 'goal') inspect('goal');
}

function animateChart(explicit = false) {
  if (!explicit && motion.matches) return;
  for (const mark of ui.plot.querySelectorAll('[data-reveal]')) {
    for (const animation of mark.getAnimations()) animation.cancel();
    const reveal = mark.dataset.reveal;
    let keyframes = [{ transform: 'scaleX(0)', opacity: .48 }, { transform: 'scaleX(1)', opacity: 1 }];
    if (reveal === 'point') keyframes = [{ opacity: .22 }, { opacity: 1 }];
    if (reveal === 'ring') keyframes = [
      { strokeDashoffset: '100' }, { strokeDashoffset: mark.getAttribute('stroke-dashoffset') },
    ];
    if (reveal === 'line') keyframes = [{ strokeDashoffset: '1' }, { strokeDashoffset: '0' }];
    mark.animate(keyframes, {
      duration: reveal === 'point' ? 300 : reveal === 'line' ? 1050 : 680,
      delay: Number(mark.dataset.delay ?? 0),
      easing: 'cubic-bezier(.25,.08,.24,1)',
      fill: 'backwards',
    });
  }
  if (explicit) ui.replay.firstElementChild.animate(
    [{ transform: 'rotate(0deg)' }, { transform: 'rotate(360deg)' }],
    { duration: 450, easing: 'ease-out' },
  );
}

function renderChart() {
  readings.clear();
  renderContext();
  const resourceSamples = view.profile.resources?.samples;
  const hardwareCaption = resourceSamples === undefined ? 'Hardware readings for the full session.' :
    number(resourceSamples, 0) + ' samples across the full session, including loading.';
  const display = {
    compare: ['Compare console profiles', 'The profiles share a scale in each chart. Change the target to move the gold markers.', makeComparison, 'fps-0'],
    fps: ['Frame rate in the world', 'Drag the gold marker to compare the samples with a different target.', makeFrameRate, 'median'],
    timing: view.mode === 'time' ?
      ['Frame timing', 'Separate medians for each timing counter.', makeFrameTiming, 'frame'] :
      ['Data transferred per frame', 'Median reported data volume by counter.', makeFrameTiming, 'copy'],
    hardware: ['GPU and memory', hardwareCaption, makeHardware, 'vram'],
    session: ['Frame rate over the session', 'Move across the chart or use the slider to inspect a recorded minute.', makeSession, 'minute-' + (view.session?.minutes.length - 1)],
  };
  const [title, caption, makePlot, initial] = display[view.chart];
  byId('chart-title').textContent = title;
  byId('chart-caption').textContent = caption;
  ui.modes.hidden = view.chart !== 'timing';
  ui.plot.replaceChildren(makePlot());
  const reading = ui.plot.nextElementSibling;
  reading.hidden = !readings.size;
  ui.replay.hidden = !readings.size;
  for (const control of ui.modes.querySelectorAll('[data-mode]')) {
    control.setAttribute('aria-pressed', String(control.dataset.mode === view.mode));
  }
  view.selected = '';
  updateTarget(view.target);
  inspect(initial);
  animateChart();
}

function selectChart(name, focus = false) {
  view.chart = name;
  for (const tab of ui.tabs) {
    const active = tab.dataset.chart === name;
    tab.setAttribute('aria-selected', String(active));
    tab.tabIndex = active ? 0 : -1;
    if (active) {
      ui.panel.setAttribute('aria-labelledby', tab.id);
      if (focus) tab.focus();
    }
  }
  renderChart();
}

function renderContext() {
  const { benchmark: data, profile, session } = view;
  const comparing = view.chart === 'compare';
  const playLog = view.chart === 'session';
  byId('overview').hidden = comparing || playLog;
  byId('profile-field').hidden = comparing || playLog || data.profiles.length < 2;
  byId('target-panel').hidden = playLog;
  byId('dashboard-grid').classList.toggle('session-view', playLog);
  if (playLog) {
    byId('benchmark-context').replaceChildren(
      session.profile + ' · Play log · Build ', element('code', '', session.build),
      ' · ' + session.minutes.length + ' recorded minutes',
    );
    return;
  }
  const date = new Intl.DateTimeFormat('en-GB', {
    day: 'numeric', month: 'short', year: 'numeric', timeZone: 'UTC',
  }).format(new Date(data.date + 'T00:00:00Z'));
  byId('benchmark-context').replaceChildren(
    (comparing ? 'RTX 4090 · Build ' : profile.label + ' · RTX 4090 · Build '),
    element('code', '', data.build), ' · ' + date,
    comparing ? ' · ' + data.profiles.length + ' measured profiles' : ' · ' + number(profile.samples, 0) + ' in-world samples',
  );
}

function renderProfile(profile) {
  view.profile = profile;
  showValue('stat-fps', profile.fps.median, 'fps');
  showValue('stat-low', profile.fps.low, 'fps');
  showValue('stat-frame', profile.timing.frame, 'ms');
  showValue('stat-gpu', profile.resources?.load ?? null, '%', 0);
  selectChart(view.chart);
}

function measurementsTable(profiles) {
  const body = byId('measurement-rows');
  for (const profile of profiles) {
    const { fps, timing, resources } = profile;
    const row = element('tr');
    const name = element('th', '', profile.label);
    name.scope = 'row';
    row.append(name);
    for (const cell of [
      number(profile.samples, 0), number(fps.median), number(fps.p10) + '–' + number(fps.p90),
      number(fps.low), number(timing.frame) + ' ms', resources && number(resources.load, 0) + '%',
      resources && number(resources.vram) + ' GB',
    ]) row.append(element('td', '', cell ?? '—'));
    body.append(row);
  }
}

function bindTargetMarker() {
  ui.plot.addEventListener('pointerdown', event => {
    const marker = event.target.closest('.pin-goal');
    if (!marker || event.button !== 0) return;
    marker.setPointerCapture(event.pointerId);
    marker.parentElement.classList.add('dragging');
    inspect('goal');
  });
  ui.plot.addEventListener('pointermove', event => {
    const marker = event.target.closest('.pin-goal');
    if (!marker?.hasPointerCapture(event.pointerId)) return;
    const { left, width } = marker.parentElement.getBoundingClientRect();
    const value = Math.round((event.clientX - left) / width * view.limit);
    updateTarget(value);
  });
  ui.plot.addEventListener('lostpointercapture', event => {
    event.target.closest('.plot-track')?.classList.remove('dragging');
  });
  ui.plot.addEventListener('keydown', event => {
    if (!event.target.closest('.pin-goal')) return;
    const step = { ArrowLeft: -1, ArrowRight: 1 }[event.key];
    if (!step) return;
    event.preventDefault();
    updateTarget(view.target + step);
  });
}

async function start() {
  const sessionRequest = fetch(new URL('./play-log.md', import.meta.url), { cache: 'no-cache' })
    .then(async response => response.ok ? parsePlayLog(await response.text()) : null)
    .catch(() => null);
  const source = await fetch(new URL('./measurements.md', import.meta.url), { cache: 'no-cache' });
  if (!source.ok) throw new Error('Measurement source returned HTTP ' + source.status);
  const data = parseBenchmark(await source.text());
  view.benchmark = data;
  view.session = await sessionRequest;
  byId('compare-tab').hidden = data.profiles.length < 2;
  byId('session-tab').hidden = !view.session;
  const activeTabs = ui.tabs.filter(tab => !tab.hidden);
  const requested = location.hash.slice(1);
  view.chart = activeTabs.find(tab => tab.dataset.chart === requested)?.dataset.chart ?? activeTabs[0].dataset.chart;
  for (const profile of data.profiles) ui.profile.add(new Option(profile.label, profile.name));
  ui.profile.addEventListener('change', () => {
    renderProfile(data.profiles.find(profile => profile.name === ui.profile.value));
  });
  for (const tab of ui.tabs) {
    tab.addEventListener('click', () => selectChart(tab.dataset.chart));
    tab.addEventListener('keydown', event => {
      const index = activeTabs.indexOf(tab);
      const last = activeTabs.length - 1;
      const next = { ArrowLeft: index === 0 ? last : index - 1, ArrowRight: index === last ? 0 : index + 1,
        Home: 0, End: last }[event.key];
      if (next === undefined) return;
      event.preventDefault();
      selectChart(activeTabs[next].dataset.chart, true);
    });
  }
  for (const type of ['pointerover', 'click', 'focusin']) {
    ui.plot.addEventListener(type, event => {
      const key = event.target.closest('button[data-inspect]')?.dataset.inspect;
      if (key && key !== view.selected) inspect(key);
    });
  }
  ui.modes.addEventListener('click', event => {
    const mode = event.target.closest('button[data-mode]')?.dataset.mode;
    if (mode && mode !== view.mode) {
      view.mode = mode;
      renderChart();
    }
  });
  ui.slider.addEventListener('input', () => updateTarget(ui.slider.valueAsNumber));
  for (const preset of ui.presets) preset.addEventListener('click', () => updateTarget(Number(preset.dataset.target)));
  ui.replay.addEventListener('click', () => animateChart(true));
  bindTargetMarker();
  measurementsTable(data.profiles);
  renderProfile(data.profiles[0]);
  byId('load-status').hidden = true;
  ui.dashboard.hidden = false;
}

start().catch(error => {
  byId('load-status').textContent = 'The dashboard could not load the measurements. Open Measurement notes to view the source data.';
  console.error(error);
});
