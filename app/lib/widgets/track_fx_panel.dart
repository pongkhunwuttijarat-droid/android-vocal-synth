import 'package:flutter/material.dart';

import '../models.dart';
import '../theme.dart';

/// Editor for the selected track's persisted OpenUtau UMixFx value.
/// Changes remain a disposable draft until the user applies them to the Rust
/// project session.
class TrackFxPanel extends StatefulWidget {
  const TrackFxPanel({super.key, required this.track, required this.onApply});

  final Track? track;
  final Future<void> Function(Map<String, dynamic>? value) onApply;

  @override
  State<TrackFxPanel> createState() => _TrackFxPanelState();
}

class _TrackFxPanelState extends State<TrackFxPanel> {
  late Map<String, dynamic> _value;
  bool _saving = false;

  static const _defaults = <String, dynamic>{
    'enabled': false,
    'eq_preset': 'vocal_air',
    'comp_preset': 'gentle',
    'reverb_preset': 'small_room',
    'eq_low_db': 0.0,
    'eq_mid_freq': 3000.0,
    'eq_mid_db': 1.5,
    'eq_high_db': 3.0,
    'comp_threshold_db': -18.0,
    'comp_ratio': 2.0,
    'comp_makeup_db': 2.5,
    'reverb_size': 0.3,
    'reverb_damp': 0.7,
    'reverb_wet': 1.0,
    'reverb_pre_delay_ms': 0.0,
  };

  @override
  void initState() {
    super.initState();
    _resetValue();
  }

  @override
  void didUpdateWidget(covariant TrackFxPanel oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.track != widget.track) _resetValue();
  }

  void _resetValue() {
    _value = {..._defaults, ...?widget.track?.mixFx};
  }

  void _set(String key, Object value) => setState(() => _value[key] = value);

  void _choosePreset(String key, String preset) {
    setState(() {
      _value[key] = preset;
      if (key == 'eq_preset') {
        final values = switch (preset) {
          'vocal_air' => [0.0, 3000.0, 1.5, 3.0],
          'warm' => [2.0, 500.0, 0.0, -1.0],
          'demud' => [0.0, 200.0, -3.0, 0.0],
          'telephone' => [-6.0, 1500.0, 4.0, -6.0],
          _ => [0.0, 1000.0, 0.0, 0.0],
        };
        _value.addAll({
          'eq_low_db': values[0],
          'eq_mid_freq': values[1],
          'eq_mid_db': values[2],
          'eq_high_db': values[3],
        });
      } else if (key == 'comp_preset') {
        final values = switch (preset) {
          'gentle' => [-18.0, 2.0, 2.5],
          'pop' => [-14.0, 3.0, 4.0],
          'limit' => [-3.0, 10.0, 0.0],
          _ => [0.0, 1.0, 0.0],
        };
        _value.addAll({
          'comp_threshold_db': values[0],
          'comp_ratio': values[1],
          'comp_makeup_db': values[2],
        });
      } else if (key == 'reverb_preset') {
        final values = switch (preset) {
          'small_room' => [0.30, 0.7, 1.0, 12.0],
          'vocal_plate' => [0.55, 0.3, 1.0, 25.0],
          'hall' => [0.85, 0.4, 1.0, 40.0],
          'ambient' => [0.92, 0.2, 1.0, 60.0],
          _ => [0.0, 0.0, 0.0, 0.0],
        };
        _value.addAll({
          'reverb_size': values[0],
          'reverb_damp': values[1],
          'reverb_wet': values[2],
          'reverb_pre_delay_ms': values[3],
        });
      }
    });
  }

  Widget _slider(
    String label,
    String key,
    double min,
    double max, {
    String suffix = '',
    int divisions = 100,
  }) {
    final value = (_value[key] as num?)?.toDouble() ?? min;
    return SizedBox(
      width: 230,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            '$label  ${value.toStringAsFixed(1)}$suffix',
            style: const TextStyle(color: RustedUtauColors.text, fontSize: 11),
          ),
          Slider(
            value: value.clamp(min, max).toDouble(),
            min: min,
            max: max,
            divisions: divisions,
            onChanged: (v) => _set(key, v),
          ),
        ],
      ),
    );
  }

  Widget _preset(String label, String key, List<String> values) {
    final current = values.contains(_value[key])
        ? _value[key] as String
        : values.first;
    return SizedBox(
      width: 148,
      child: DropdownButtonFormField<String>(
        value: current,
        decoration: InputDecoration(
          labelText: label,
          isDense: true,
          filled: true,
          fillColor: const Color(0xFF252730),
          border: OutlineInputBorder(borderRadius: BorderRadius.circular(6)),
        ),
        items: [
          for (final value in values)
            DropdownMenuItem(value: value, child: Text(value)),
        ],
        onChanged: (value) {
          if (value != null) _choosePreset(key, value);
        },
      ),
    );
  }

  Future<void> _save(Map<String, dynamic>? value) async {
    setState(() => _saving = true);
    try {
      await widget.onApply(value);
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final track = widget.track;
    return Container(
      decoration: const BoxDecoration(
        color: Color(0xFF15151A),
        border: Border(top: BorderSide(color: RustedUtauColors.line)),
      ),
      padding: const EdgeInsets.fromLTRB(14, 8, 14, 12),
      height: 260,
      child: track == null
          ? const Text(
              'Select a track to edit its effects.',
              style: TextStyle(color: RustedUtauColors.muted),
            )
          : Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Text(
                      'TRACK FX · ${track.name}',
                      style: const TextStyle(
                        fontSize: 11,
                        letterSpacing: 0.5,
                        color: RustedUtauColors.text,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                    const SizedBox(width: 12),
                    const Text(
                      'Enabled',
                      style: TextStyle(color: RustedUtauColors.muted, fontSize: 11),
                    ),
                    Switch(
                      value: _value['enabled'] as bool? ?? false,
                      onChanged: (v) => _set('enabled', v),
                    ),
                    const Spacer(),
                    TextButton(
                      onPressed: _saving ? null : () => _save(null),
                      child: const Text('Clear'),
                    ),
                    const SizedBox(width: 4),
                    FilledButton(
                      onPressed: _saving
                          ? null
                          : () => _save(Map<String, dynamic>.from(_value)),
                      child: Text(_saving ? 'Saving…' : 'Apply'),
                    ),
                  ],
                ),
                Expanded(
                  child: SingleChildScrollView(
                    scrollDirection: Axis.horizontal,
                    child: Row(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        _preset('EQ preset', 'eq_preset', const [
                          'off',
                          'vocal_air',
                          'warm',
                          'demud',
                          'telephone',
                        ]),
                        const SizedBox(width: 10),
                        _slider('Low', 'eq_low_db', -24, 24, suffix: ' dB'),
                        _slider(
                          'Mid frequency',
                          'eq_mid_freq',
                          20,
                          20000,
                          suffix: ' Hz',
                        ),
                        _slider('Mid', 'eq_mid_db', -24, 24, suffix: ' dB'),
                        _slider('High', 'eq_high_db', -24, 24, suffix: ' dB'),
                        const SizedBox(width: 10),
                        _preset('Compressor', 'comp_preset', const [
                          'off',
                          'gentle',
                          'pop',
                          'limit',
                        ]),
                        _slider(
                          'Threshold',
                          'comp_threshold_db',
                          -60,
                          0,
                          suffix: ' dB',
                        ),
                        _slider('Ratio', 'comp_ratio', 1, 20, suffix: ':1'),
                        _slider(
                          'Makeup',
                          'comp_makeup_db',
                          -24,
                          24,
                          suffix: ' dB',
                        ),
                        const SizedBox(width: 10),
                        _preset('Reverb', 'reverb_preset', const [
                          'off',
                          'small_room',
                          'vocal_plate',
                          'hall',
                          'ambient',
                        ]),
                        _slider('Size', 'reverb_size', 0, 1),
                        _slider('Damping', 'reverb_damp', 0, 1),
                        _slider('Wet', 'reverb_wet', 0, 2),
                        _slider(
                          'Pre-delay',
                          'reverb_pre_delay_ms',
                          0,
                          200,
                          suffix: ' ms',
                        ),
                      ],
                    ),
                  ),
                ),
              ],
            ),
    );
  }
}
