export function parseBenchmark(text) {
  const metadata = text.match(/^Build\s+([a-f0-9]+)\b.*?\b(\d{4}-\d{2}-\d{2})\b/m);
  if (!metadata) throw new Error('The benchmark build and date are missing.');

  const lines = text.split(/\r?\n/).map(line => line.trim());
  const profiles = readTable(lines, 'world lines');
  const resources = readTable(lines, 'samples');
  if (profiles.size === 0) throw new Error('The benchmark has no measured profiles.');

  return {
    build: metadata[1],
    date: metadata[2],
    profiles: [...profiles].map(([name, fields]) => {
      const [samples, median, p10, p90, low, frame, prepare, copies, textures, gpu, back] = fields;
      const [copyMs, copyMb] = readPair(copies);
      const [textureMs, textureMb] = readPair(textures);
      const [backMs, backMb] = readPair(back);
      return {
        name,
        samples: readNumber(samples),
        fps: {
          median: readNumber(median),
          p10: readNumber(p10),
          p90: readNumber(p90),
          low: readNumber(low),
        },
        timing: {
          frame: readNumber(frame),
          prepare: readNumber(prepare),
          gpu: readNumber(gpu),
          copy: copyMs,
          texture: textureMs,
          back: backMs,
        },
        transfers: { copy: copyMb, texture: textureMb, back: backMb },
        resources: resources.has(name) ? readResources(resources.get(name)) : null,
      };
    }),
  };
}

export function parsePlayLog(text) {
  const profile = text.match(/^# Play log:\s*([^,\r\n]+)/m)?.[1].trim();
  const build = text.match(/^Build\s+([a-f0-9]+)\b/m)?.[1];
  if (!profile || !build) throw new Error('The play log profile or build is missing.');

  const minutes = [];
  let previous = 0;
  for (const line of text.split(/\r?\n/)) {
    const [time, scene, fps, preparing, copyingBack] = cells(line.trim());
    const match = time?.match(/^(\d{2}):(\d{2})$/);
    if (!match) continue;
    const hours = Number(match[1]);
    const minute = Number(match[2]);
    if (hours > 23 || minute > 59) throw new Error('A play log timestamp is invalid.');
    let elapsed = hours * 60 + minute + Math.floor(previous / 1440) * 1440;
    if (elapsed < previous) elapsed += 1440;
    if (minutes.length && elapsed === previous) throw new Error('A play log minute is listed twice.');
    minutes.push({
      time, elapsed, scene, fps: readNumber(fps),
      preparing: readNumber(preparing), copyingBack: readNumber(copyingBack),
    });
    previous = elapsed;
  }
  if (!minutes.length) throw new Error('The play log has no per-minute samples.');
  return { profile, build, minutes };
}

function readTable(lines, secondColumn) {
  const rows = new Map();
  const header = lines.findIndex(line => {
    const [profile, unit] = cells(line);
    return profile === 'Profile' && unit === secondColumn;
  });
  if (header === -1) return rows;
  for (const line of lines.slice(header + 2)) {
    const [profile, ...values] = cells(line);
    if (!profile) break;
    if (rows.has(profile)) throw new Error('A benchmark profile is listed twice.');
    rows.set(profile, values);
  }
  return rows;
}

function cells(line) {
  return line.startsWith('|') ? line.slice(1, -1).split('|').map(cell => cell.trim()) : [];
}

function readNumber(value) {
  const number = Number(value?.replaceAll(',', ''));
  if (!value || !Number.isFinite(number)) throw new Error('A benchmark measurement is missing or invalid.');
  return number;
}

function readPair(value) {
  const [first, second] = value?.split('/').map(item => item.trim()) ?? [];
  return [readNumber(first), readNumber(second)];
}

function readResources(fields) {
  const [samples, memory, workingSet, vram, load, power, temperature, clock] = fields;
  const [ramMedian, ramMax] = readPair(memory);
  return {
    samples: readNumber(samples),
    ramMedian,
    ramMax,
    workingSet: readNumber(workingSet),
    vram: readNumber(vram),
    load: readNumber(load),
    power: readNumber(power),
    temperature: readNumber(temperature),
    clock: readNumber(clock),
  };
}
