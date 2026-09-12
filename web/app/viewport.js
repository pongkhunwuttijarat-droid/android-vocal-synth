// Viewport: beat/pitch ↔ pixel math for the piano roll. Pure functions plus a
// small stateful object, so it is testable without a DOM.
//
// The document's tick/timing rules live in Rust; this module only maps the
// tick values it receives onto the canvas.

/** Vertical size of one semitone row, before zoom (px). */
export const BASE_ROW_H = 18;
/** Horizontal size of one beat, before zoom (px). */
export const BASE_BEAT_W = 96;
/** Pitch rows drawn above/below the note range. */
export const PITCH_PAD = 6;

/** Ticks per quarter note in an OpenUtau project. */
export const TICKS_PER_BEAT = 480;

export class Viewport {
  constructor() {
    this.rowH = BASE_ROW_H;
    this.beatW = BASE_BEAT_W;
    /** Scroll offset in px. */
    this.panX = 0;
    this.panY = 0;
    this.width = 0;
    this.height = 0;
    /** Lowest and highest tone present, for the initial scroll. */
    this.minTone = 48;
    this.maxTone = 72;
  }

  resize(width, height) {
    this.width = width;
    this.height = height;
  }

  /** Fit the note range into view and scroll to the song start. */
  frameNotes(notes) {
    if (!notes.length) return;
    let lo = Infinity;
    let hi = -Infinity;
    for (const n of notes) {
      lo = Math.min(lo, n.tone);
      hi = Math.max(hi, n.tone);
    }
    this.minTone = lo - PITCH_PAD;
    this.maxTone = hi + PITCH_PAD;
    this.panX = 0;
    this.centerOnPitch((lo + hi) / 2);
  }

  /** Scroll so `tone` (fractional) sits at the vertical centre. */
  centerOnPitch(tone) {
    const rows = this.maxTone - this.minTone + 1;
    const contentH = rows * this.rowH;
    if (contentH <= this.height) {
      this.panY = (contentH - this.height) / 2;
    } else {
      // y grows downward as tone decreases: tone -> (maxTone - tone) * rowH
      const y = (this.maxTone - tone) * this.rowH;
      this.panY = y - this.height / 2;
    }
    this.clampPan();
  }

  /** Content bounds in px. */
  contentSize() {
    return {
      w: this.beatW * 400, // generous: the grid scrolls past the song end
      h: (this.maxTone - this.minTone + 1) * this.rowH,
    };
  }

  clampPan() {
    const { w, h } = this.contentSize();
    const maxX = Math.max(0, w - this.width);
    const maxY = Math.max(0, h - this.height);
    this.panX = Math.min(Math.max(0, this.panX), maxX);
    this.panY = Math.min(Math.max(0, this.panY), maxY);
  }

  // ---- conversions -------------------------------------------------------

  /** Tick -> x px. */
  x(tick) {
    return (tick / TICKS_PER_BEAT) * this.beatW - this.panX;
  }

  /** x px -> tick (may be negative / fractional). */
  tick(x) {
    return ((x + this.panX) / this.beatW) * TICKS_PER_BEAT;
  }

  /** Tone (MIDI number) -> y px of the row's TOP edge. */
  y(tone) {
    return (this.maxTone - tone) * this.rowH - this.panY;
  }

  /** y px -> fractional tone (integer part = row under the cursor). */
  tone(y) {
    return this.maxTone - (y + this.panY) / this.rowH;
  }

  /** Row height / beat width at the current zoom. */
  rowHeight() {
    return this.rowH;
  }
  beatWidth() {
    return this.beatW;
  }

  /** Zoom around a screen point (keeps the point's musical position fixed). */
  zoomAt(x, y, factor, axis = "both") {
    if (axis === "x" || axis === "both") {
      const tickBefore = this.tick(x);
      this.beatW = clamp(this.beatW * factor, 16, 640);
      this.panX = (tickBefore / TICKS_PER_BEAT) * this.beatW - x;
    }
    if (axis === "y" || axis === "both") {
      const toneBefore = this.tone(y);
      this.rowH = clamp(this.rowH * factor, 6, 72);
      this.panY = (this.maxTone - toneBefore) * this.rowH - y;
    }
    this.clampPan();
  }

  /** Scroll by a screen delta. */
  scrollBy(dx, dy) {
    this.panX += dx;
    this.panY += dy;
    this.clampPan();
  }

  /** Visible tick range (for culling). */
  visibleTicks() {
    return { from: this.tick(0), to: this.tick(this.width) };
  }

  /** Visible tone range (for culling). */
  visibleTones() {
    return { from: Math.floor(this.tone(this.height)), to: Math.ceil(this.tone(0)) };
  }
}

export function clamp(v, lo, hi) {
  return Math.min(Math.max(lo, v), hi);
}

/** Tone -> name (C4 = 60), for key labels. */
export function toneName(tone) {
  const names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
  const n = names[((tone % 12) + 12) % 12];
  const octave = Math.floor(tone / 12) - 1;
  return `${n}${octave}`;
}

/** True for a black key (used for row shading). */
export function isBlackKey(tone) {
  return [1, 3, 6, 8, 10].includes(((tone % 12) + 12) % 12);
}

/**
 * Snap `tick` to the grid.
 *
 * The committed value still goes through Rust (the command validates bounds);
 * this is only the preview position and the value we ask for, so a plain
 * round is correct here.
 */
export function snapTick(tick, division, enabled = true) {
  if (!enabled || !division) return Math.round(tick);
  const step = TICKS_PER_BEAT / division;
  return Math.round(tick / step) * step;
}
