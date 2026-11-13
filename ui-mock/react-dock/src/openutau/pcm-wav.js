/** Encode two frame-aligned float PCM channels as a little-endian PCM16 RIFF/WAVE. */
export function encodeStereoPcm16Wav(channels, sampleRate) {
  if (!Array.isArray(channels) || channels.length !== 2) {
    throw new Error('the render did not return stereo audio');
  }
  if (!channels[0] || !channels[1] || channels[0].length !== channels[1].length) {
    throw new Error('the render returned channels with different frame counts');
  }
  if (!Number.isInteger(sampleRate) || sampleRate <= 0) {
    throw new Error('the render returned an invalid sample rate');
  }
  const frameCount = channels[0].length;
  const dataBytes = frameCount * 4;
  if (!Number.isSafeInteger(dataBytes) || dataBytes > 0xffffffff - 36) {
    throw new RangeError('the rendered WAV exceeds the RIFF 32-bit size limit');
  }
  const bytes = new ArrayBuffer(44 + dataBytes);
  const view = new DataView(bytes);
  const putTag = (offset, tag) => [...tag].forEach((char, index) => view.setUint8(offset + index, char.charCodeAt(0)));
  putTag(0, 'RIFF');
  view.setUint32(4, 36 + dataBytes, true);
  putTag(8, 'WAVE');
  putTag(12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, 2, true); // stereo
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 4, true);
  view.setUint16(32, 4, true); // block align
  view.setUint16(34, 16, true);
  putTag(36, 'data');
  view.setUint32(40, dataBytes, true);
  for (let frame = 0; frame < frameCount; frame++) {
    for (let channel = 0; channel < 2; channel++) {
      const value = Math.max(-1, Math.min(1, channels[channel][frame]));
      // PCM16 has one more negative value than positive; avoid wrapping +1 to -1.
      const scaled = value < 0 ? value * 32768 : value * 32767;
      view.setInt16(44 + (frame * 2 + channel) * 2, Math.round(scaled), true);
    }
  }
  return bytes;
}
