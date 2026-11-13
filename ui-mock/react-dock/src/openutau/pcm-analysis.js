// Presentation-only waveform envelope. Safe to run in a worker so a long
// stereo render does not make the editor's main thread scan every PCM frame.
export function buildPeaks(samples, sampleRate = 44100, bucket = 64) {
  const channels = Array.isArray(samples) ? samples : [samples];
  const frames = channels[0]?.length ?? 0;
  const count = Math.max(1, Math.ceil(frames / bucket));
  const min = new Float32Array(count);
  const max = new Float32Array(count);
  for (let b = 0; b < count; b++) {
    const start = b * bucket;
    const end = Math.min(frames, start + bucket);
    let lo = 0;
    let hi = 0;
    if (end > start) {
      lo = channels[0][start];
      hi = channels[0][start];
      for (let channel = 0; channel < channels.length; channel++) {
        const pcm = channels[channel];
        for (let i = start; i < end; i++) {
          const value = pcm[i];
          if (value < lo) lo = value;
          if (value > hi) hi = value;
        }
      }
    }
    min[b] = lo;
    max[b] = hi;
  }
  return { min, max, bucket, sampleRate, samples: frames };
}

/** Build an outer envelope directly from interleaved stereo PCM without channel copies. */
export function buildInterleavedStereoPeaks(pcm, sampleRate = 44100, bucket = 64) {
  if (!pcm || typeof pcm.length !== 'number' || pcm.length % 2 !== 0) {
    throw new TypeError('interleaved stereo PCM must contain complete frames');
  }
  const frames = pcm.length / 2;
  const count = Math.max(1, Math.ceil(frames / bucket));
  const min = new Float32Array(count);
  const max = new Float32Array(count);
  for (let b = 0; b < count; b++) {
    const start = b * bucket;
    const end = Math.min(frames, start + bucket);
    let lo = 0;
    let hi = 0;
    for (let frame = start; frame < end; frame++) {
      const left = pcm[frame * 2];
      const right = pcm[frame * 2 + 1];
      if (left < lo) lo = left;
      if (left > hi) hi = left;
      if (right < lo) lo = right;
      if (right > hi) hi = right;
    }
    min[b] = lo;
    max[b] = hi;
  }
  return { min, max, bucket, sampleRate, samples: frames };
}
