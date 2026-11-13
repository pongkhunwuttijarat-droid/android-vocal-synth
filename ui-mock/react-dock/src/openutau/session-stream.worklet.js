const STREAM_BUFFER_LOW_WATER_SECONDS = 14;
const STREAM_BUFFER_TARGET_SECONDS = 16;

class RustedUtauSessionStreamPlayer extends AudioWorkletProcessor {
  constructor() {
    super();
    this.queue = [];
    this.bufferedFrames = 0;
    this.frame = 0;
    this.startFrame = 0;
    this.started = false;
    this.finished = false;
    this.stopped = false;
    this.needData = false;
    this.lastProgressFrame = 0;
    this.underrunStartFrame = null;
    this.underrunFrames = 0;
    this.totalUnderrunFrames = 0;
    this.totalUnderrunEvents = 0;
    this.reportedFirstOutput = false;
    this.port.onmessage = ({ data }) => {
      if (data?.type === 'start') {
        this.frame = data.frame;
        this.startFrame = data.frame;
        this.startAt = data.startAt;
        this.started = true;
        this.stopped = false;
        this.finished = false;
        this.needData = false;
        this.underrunStartFrame = null;
        this.underrunFrames = 0;
        this.totalUnderrunFrames = 0;
        this.totalUnderrunEvents = 0;
        this.reportedFirstOutput = false;
      } else if (data?.type === 'push' && !this.stopped) {
        const left = data.left;
        const right = data.right;
        this.queue.push({ left, right, offset: 0, frameStart: data.frameStart });
        this.bufferedFrames += left.length;
      } else if (data?.type === 'pump-complete') {
        // The main-thread estimate can finish a pump before the worklet's
        // queue has recovered. Re-arm its edge-triggered low-water signal so
        // the next process quantum can request more PCM if needed.
        this.needData = false;
      } else if (data?.type === 'finish') {
        this.finished = true;
        this.reportUnderrun();
      } else if (data?.type === 'stop') {
        this.stopped = true;
        this.queue.length = 0;
        this.bufferedFrames = 0;
      }
    };
  }

  reportUnderrun() {
    if (this.underrunFrames <= 0) return;
    this.totalUnderrunFrames += this.underrunFrames;
    this.totalUnderrunEvents += 1;
    this.port.postMessage({
      type: 'underrun',
      frameStart: this.underrunStartFrame,
      missingFrames: this.underrunFrames,
      totalUnderrunFrames: this.totalUnderrunFrames,
      totalUnderrunEvents: this.totalUnderrunEvents,
    });
    this.underrunStartFrame = null;
    this.underrunFrames = 0;
  }

  process(_inputs, outputs) {
    const output = outputs[0];
    if (!output?.length) return true;
    for (const channel of output) channel.fill(0);
    if (this.stopped || !this.started || currentTime < this.startAt) return true;
    // Collapse a contiguous run of silent render quanta into one bounded
    // report when data resumes, rather than posting one message per quantum.
    if (this.underrunFrames > 0 && this.queue.length > 0) this.reportUnderrun();
    let written = 0;
    while (written < output[0].length && this.queue.length) {
      const block = this.queue[0];
      let skip = Math.max(0, this.startFrame - block.frameStart - block.offset);
      if (skip) { block.offset += skip; this.bufferedFrames -= skip; }
      if (block.offset >= block.left.length) { this.queue.shift(); continue; }
      const count = Math.min(output[0].length - written, block.left.length - block.offset);
      for (let channel = 0; channel < output.length; channel++) {
        const samples = channel === 0 ? block.left : block.right;
        output[channel].set(samples.subarray(block.offset, block.offset + count), written);
      }
      block.offset += count;
      this.bufferedFrames -= count;
      this.frame += count;
      this.startFrame = this.frame;
      written += count;
      if (block.offset >= block.left.length) this.queue.shift();
    }
    if (written > 0 && !this.reportedFirstOutput) {
      this.reportedFirstOutput = true;
      this.port.postMessage({ type: 'first-audio-output', frame: this.frame - written, frameCount: written });
    }
    if (written < output[0].length && !this.finished) {
      if (this.underrunStartFrame === null) this.underrunStartFrame = this.frame;
      this.underrunFrames += output[0].length - written;
    }
    if (this.frame - this.lastProgressFrame >= 1024) {
      this.lastProgressFrame = this.frame;
      this.port.postMessage({ type: 'progress', frame: this.frame, bufferedFrames: this.bufferedFrames, contextTime: currentTime + output[0].length / sampleRate });
    }
    if (this.bufferedFrames < sampleRate * STREAM_BUFFER_LOW_WATER_SECONDS && !this.finished && !this.needData) {
      this.needData = true;
      this.port.postMessage({ type: 'need-data', bufferedFrames: this.bufferedFrames });
    }
    if (this.bufferedFrames >= sampleRate * STREAM_BUFFER_TARGET_SECONDS) this.needData = false;
    if (this.finished && this.queue.length === 0 && !this.stopped) {
      this.stopped = true;
      this.port.postMessage({ type: 'ended', frame: this.frame, contextTime: currentTime });
    }
    return true;
  }
}

registerProcessor('rusted-utau-session-stream-player', RustedUtauSessionStreamPlayer);
