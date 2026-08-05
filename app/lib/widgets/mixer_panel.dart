import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../models.dart';
import '../theme.dart';

/// Mixer bottom panel — mirrors ui-mock/index.html's draft-1 panel:
/// one channel strip per track (color tag, M/S, volume fader, pan knob,
/// 3-band EQ) plus a master strip. Values are local state; every change
/// is reported through [onParamsChanged] as a mixer-FX params JSON string
/// (the editor forwards it to the engine's `POST /mixer-params`).
class MixerPanel extends StatefulWidget {
  const MixerPanel({
    super.key,
    required this.tracks,
    this.onParamsChanged,
  });

  final List<Track> tracks;

  /// Called with the mixer-FX params JSON (e.g.
  /// `{"gain":0.8,"low_gain":3}`) whenever a strip value changes.
  final ValueChanged<String>? onParamsChanged;

  @override
  State<MixerPanel> createState() => _MixerPanelState();
}

class _MixerPanelState extends State<MixerPanel> {
  static const double _minDb = -24;
  static const double _maxDb = 12;

  late final List<_StripState> _strips = [
    for (final t in widget.tracks) _StripState(color: _trackColor(t.colorSeed)),
    _StripState(color: LiltColors.purple, isMaster: true),
  ];

  static Color _trackColor(int seed) =>
      LiltColors.trackColors[seed % LiltColors.trackColors.length];

  /// dB → linear gain (the C++ mixer's `gain` param is LINEAR, not dB:
  /// 0 dB → 1.0, +12 dB → ~4.0, -12 dB → ~0.25). The old `dB/20` mapping
  /// was wrong — 0 dB became 0.0 (silence!) and negative dB inverted the
  /// phase ("เสียง level ไม่ต่าง").
  static double _dbToLinear(double db) =>
      db <= -60.0 ? 0.0 : math.pow(10.0, db / 20.0).toDouble();

  /// Serialize the panel state to mixer-FX params JSON and report it.
  /// The first (non-master) strip maps to the engine's master mixer:
  /// fader → gain (dB→linear), EQ bands → dB, pan reserved.
  void _emitParams() {
    final s = _strips.first;
    final master = _strips.last;
    final gainDb = s.muted ? -60.0 : s.volume;
    widget.onParamsChanged?.call(
      '{"gain":${_dbToLinear(gainDb).toStringAsFixed(4)},'
      '"low_gain":${s.low.toStringAsFixed(1)},'
      '"mid_gain":${s.mid.toStringAsFixed(1)},'
      '"high_gain":${s.high.toStringAsFixed(1)},'
      '"eq_enabled":${s.low != 0 || s.mid != 0 || s.high != 0},'
      '"master_gain":${_dbToLinear(master.volume).toStringAsFixed(4)}}',
    );
  }

  @override
  Widget build(BuildContext context) {
    return Container(
      decoration: const BoxDecoration(
        color: Color(0xFF15151A),
        border: Border(top: BorderSide(color: LiltColors.line)),
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          _buildHeader(),
          SizedBox(
            height: 216,
            child: ListView(
              scrollDirection: Axis.horizontal,
              children: [
                for (var i = 0; i < _strips.length; i++)
                  _buildStrip(_strips[i], i),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildHeader() {
    return Container(
      height: 36,
      padding: const EdgeInsets.symmetric(horizontal: 14),
      decoration: const BoxDecoration(
        color: Color(0xFF17181F),
        border: Border(bottom: BorderSide(color: LiltColors.line)),
      ),
      child: Row(
        children: [
          const Text(
            'MIXER',
            style: TextStyle(
              fontSize: 11,
              letterSpacing: 0.06,
              color: Color(0xFFB5B9C8),
              fontWeight: FontWeight.w700,
            ),
          ),
          const SizedBox(width: 12),
          const Text(
            'Preset',
            style: TextStyle(fontSize: 10, color: LiltColors.muted),
          ),
          const SizedBox(width: 6),
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
            decoration: BoxDecoration(
              color: const Color(0xFF252833),
              border: Border.all(color: const Color(0xFF373B4B)),
              borderRadius: BorderRadius.circular(5),
            ),
            child: const Text(
              'Default',
              style: TextStyle(fontSize: 11, color: LiltColors.text),
            ),
          ),
          const Spacer(),
          for (final (label, on) in const [
            ('EQ', true),
            ('Comp', true),
            ('SoftClip', true),
          ])
            Padding(
              padding: const EdgeInsets.only(left: 5),
              child: Container(
                padding:
                    const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
                decoration: BoxDecoration(
                  color: on ? const Color(0xFF2A2642) : const Color(0xFF242733),
                  border: Border.all(
                    color: on ? const Color(0xFF765DD1) : const Color(0xFF343848),
                  ),
                  borderRadius: BorderRadius.circular(4),
                ),
                child: Text(
                  label,
                  style: TextStyle(
                    fontSize: 10,
                    color: on ? const Color(0xFFCFC4FF) : LiltColors.muted,
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }

  Widget _buildStrip(_StripState s, int index) {
    final isMaster = s.isMaster;
    final name = isMaster
        ? 'MASTER'
        : (index < widget.tracks.length ? widget.tracks[index].name : '');
    final pct = ((s.volume - _minDb) / (_maxDb - _minDb)).clamp(0.0, 1.0);
    return Container(
      width: 132,
      padding: const EdgeInsets.fromLTRB(8, 6, 8, 4),
      decoration: BoxDecoration(
        color: isMaster ? const Color(0xFF1D1D22) : LiltColors.panel,
        border: Border(
          left: isMaster ? const BorderSide(color: LiltColors.purple, width: 2) : BorderSide.none,
          right: const BorderSide(color: LiltColors.line),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // color tag + name + M/S
          Row(
            children: [
              Container(
                width: 4,
                height: 24,
                decoration: BoxDecoration(
                  color: s.color,
                  borderRadius: BorderRadius.circular(2),
                ),
              ),
              const SizedBox(width: 5),
              Expanded(
                child: Text(
                  name,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 10,
                    fontWeight: isMaster ? FontWeight.w700 : FontWeight.w600,
                    color: isMaster ? LiltColors.purple : LiltColors.text,
                  ),
                ),
              ),
              _msButton('M', s.muted, const Color(0xFFE5484D), () {
                setState(() {
                  s.muted = !s.muted;
                  _emitParams();
                });
              }),
              const SizedBox(width: 2),
              _msButton('S', s.solo, const Color(0xFFFFD93D), () {
                setState(() {
                  s.solo = !s.solo;
                  _emitParams();
                });
              }),
            ],
          ),
          // volume fader
          Expanded(
            child: Row(
              children: [
                // fader track
                GestureDetector(
                  onVerticalDragUpdate: (d) {
                    setState(() {
                      s.volume =
                          (s.volume - d.delta.dy / 2.2).clamp(_minDb, _maxDb);
                      _emitParams();
                    });
                  },
                  child: Container(
                    width: 4,
                    margin: const EdgeInsets.symmetric(vertical: 8),
                    decoration: BoxDecoration(
                      color: const Color(0xFF252730),
                      borderRadius: BorderRadius.circular(2),
                    ),
                    child: Align(
                      alignment: Alignment.bottomCenter,
                      child: Container(
                        width: 4,
                        height: 96 * pct,
                        decoration: BoxDecoration(
                          color: s.color.withValues(alpha: 0.35),
                          borderRadius: BorderRadius.circular(2),
                        ),
                      ),
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        '${s.volume >= 0 ? '+' : ''}${s.volume.toStringAsFixed(1)} dB',
                        style: const TextStyle(
                          fontSize: 10,
                          fontWeight: FontWeight.w600,
                          fontFeatures: [FontFeature.tabularFigures()],
                        ),
                      ),
                      const SizedBox(height: 4),
                      // pan knob (stylized)
                      Container(
                        width: 30,
                        height: 30,
                        decoration: BoxDecoration(
                          shape: BoxShape.circle,
                          border: Border.all(color: const Color(0xFF2A2C36)),
                          gradient: SweepGradient(
                            startAngle: -3 * 3.14159 / 4,
                            endAngle: 3 * 3.14159 / 4,
                            colors: const [
                              Color(0xFF252730),
                              LiltColors.cyan,
                              Color(0xFF252730),
                            ],
                            stops: [0.0, (s.pan + 1) / 2, 1.0],
                            transform: const GradientRotation(-3.14159 / 2),
                          ),
                        ),
                        child: Transform.rotate(
                          angle: s.pan * 135 * 3.14159 / 180,
                          child: const Icon(
                            Icons.remove,
                            size: 16,
                            color: LiltColors.text,
                          ),
                        ),
                      ),
                      const SizedBox(height: 2),
                      Text(
                        s.pan == 0
                            ? 'C'
                            : '${s.pan > 0 ? 'R' : 'L'} ${(s.pan.abs() * 100).round()}%',
                        style: const TextStyle(
                          fontSize: 9,
                          color: LiltColors.muted,
                        ),
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
          // EQ knobs
          Row(
            children: [
              for (final (band, db) in [
                ('LOW', s.low),
                ('MID', s.mid),
                ('HIGH', s.high),
              ])
                Expanded(
                  child: _EqKnob(
                    label: band,
                    db: db,
                    onChanged: (v) => setState(() {
                      if (band == 'LOW') s.low = v;
                      if (band == 'MID') s.mid = v;
                      if (band == 'HIGH') s.high = v;
                      _emitParams();
                    }),
                  ),
                ),
            ],
          ),
          if (!isMaster) ...[
            const SizedBox(height: 3),
            const Row(
              children: [
                Expanded(
                  child: Text(
                    'Thresh',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(fontSize: 8, color: LiltColors.muted),
                  ),
                ),
                Text(
                  '-12dB',
                  style: TextStyle(fontSize: 8, color: LiltColors.muted),
                ),
              ],
            ),
            const SizedBox(height: 3),
            Container(
              height: 4,
              decoration: BoxDecoration(
                color: const Color(0xFF252730),
                borderRadius: BorderRadius.circular(2),
              ),
              child: FractionallySizedBox(
                alignment: Alignment.centerLeft,
                widthFactor: 0.4,
                child: Container(
                  decoration: BoxDecoration(
                    borderRadius: BorderRadius.circular(2),
                    gradient: const LinearGradient(colors: [
                      LiltColors.green,
                      LiltColors.orange,
                      Color(0xFFFF6B6B),
                    ]),
                  ),
                ),
              ),
            ),
          ] else
            const Padding(
              padding: EdgeInsets.only(top: 3),
              child: Text(
                'MASTER EQ',
                style: TextStyle(fontSize: 8, color: LiltColors.muted),
              ),
            ),
        ],
      ),
    );
  }

  Widget _msButton(String label, bool on, Color onColor, VoidCallback onTap) {
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(3),
      child: Container(
        width: 20,
        height: 16,
        alignment: Alignment.center,
        decoration: BoxDecoration(
          color: on ? onColor : const Color(0xFF252730),
          borderRadius: BorderRadius.circular(3),
        ),
        child: Text(
          label,
          style: TextStyle(
            fontSize: 9,
            fontWeight: FontWeight.w700,
            color: on ? const Color(0xFF111111) : const Color(0xFF555555),
          ),
        ),
      ),
    );
  }
}

class _StripState {
  _StripState({required this.color, this.isMaster = false});
  final Color color;
  final bool isMaster;
  double volume = 0;
  double pan = 0;
  double low = 0;
  double mid = 0;
  double high = 0;
  bool muted = false;
  bool solo = false;
}

class _EqKnob extends StatelessWidget {
  const _EqKnob({
    required this.label,
    required this.db,
    required this.onChanged,
  });

  final String label;
  final double db;
  final ValueChanged<double> onChanged;

  @override
  Widget build(BuildContext context) {
    final pct = ((db + 12) / 24).clamp(0.0, 1.0);
    return GestureDetector(
      onVerticalDragUpdate: (d) {
        onChanged((db - d.delta.dy / 2.0).clamp(-12.0, 12.0));
      },
      child: Column(
        children: [
          Container(
            width: 20,
            height: 20,
            decoration: BoxDecoration(
              shape: BoxShape.circle,
              border: Border.all(color: const Color(0xFF2A2C36)),
              gradient: SweepGradient(
                startAngle: -3 * 3.14159 / 4,
                endAngle: 3 * 3.14159 / 4,
                colors: const [
                  Color(0xFF252730),
                  LiltColors.green,
                  Color(0xFF252730),
                ],
                stops: [0.0, pct, 1.0],
                transform: const GradientRotation(-3.14159 / 2),
              ),
            ),
            child: Transform.rotate(
              angle: (db / 12) * 135 * 3.14159 / 180,
              child: const Icon(Icons.remove, size: 12, color: LiltColors.text),
            ),
          ),
          Text(
            label,
            style: const TextStyle(fontSize: 8, color: LiltColors.muted),
          ),
        ],
      ),
    );
  }
}
