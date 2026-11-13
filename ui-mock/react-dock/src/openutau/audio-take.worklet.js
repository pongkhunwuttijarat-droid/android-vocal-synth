class RustedUtauTakePlayer extends AudioWorkletProcessor {
  constructor() {
    super();
    this.channels = [];
    this.frame = 0;
    this.startAt = 0;
    this.stopped = false;
    this.ended = false;
    this.lastProgressFrame = 0;
    this.reportedFirstOutput = false;
    this.port.onmessage = ({ data }) => {
      if (data?.type === 'stop') {
        this.stopped = true;
        return;
      }
      if (data?.type === 'start') {
        this.channels = data.channels || [];
        this.frame = data.frame || 0;
        this.startAt = data.startAt || 0;
        this.stopped = false;
        this.ended = false;
        this.lastProgressFrame = this.frame;
        this.reportedFirstOutput = false;
      }
    };
  }

  process(_inputs, outputs) {
    const output = outputs[0];
    if (!output?.length) return true;
    for (const channel of output) channel.fill(0);
    if (this.stopped || !this.channels.length || currentTime < this.startAt) return true;

    const length = this.channels[0].length;
    const outputStartFrame = this.frame;
    let written = 0;
    for (let frame = 0; frame < output[0].length; frame++) {
      if (this.frame >= length) {
        if (!this.ended) {
          this.ended = true;
          this.port.postMessage({ type: 'ended' });
        }
        this.stopped = true;
        break;
      }
      for (let channel = 0; channel < output.length; channel++) {
        output[channel][frame] = this.channels[Math.min(channel, this.channels.length - 1)][this.frame];
      }
      this.frame++;
      written++;
    }
    if (written > 0 && !this.reportedFirstOutput) {
      this.reportedFirstOutput = true;
      this.port.postMessage({ type: 'first-audio-output', frame: outputStartFrame, frameCount: written });
    }
    // Report the frame the worklet actually copied to the output. A coarse
    // cadence keeps message traffic low while still letting the UI interpolate
    // smoothly between real playback positions.
    if (this.frame - this.lastProgressFrame >= 512) {
      this.lastProgressFrame = this.frame;
      this.port.postMessage({
        type: 'progress',
        frame: this.frame,
        contextTime: currentTime + output[0].length / sampleRate,
      });
    }
    return true;
  }
}

registerProcessor('rusted-utau-take-player', RustedUtauTakePlayer);
