import 'package:flutter/material.dart';

import '../api/engine_client.dart';
import '../theme.dart';

/// App settings — separate page per the UI decision.
///
/// POC quality: every option is present and interactive, but changes are
/// local state + mock SnackBars (no persistence, no provider).
///
/// The engine server URL is kept in memory for v1 (not persisted across
/// restarts); it is applied live to the shared [ApiClient] so Voicebanks /
/// Editor use the edited URL immediately.
class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key, this.client});

  /// Engine client; defaults to a real one when not injected (tests inject
  /// a mocked client).
  final ApiClient? client;

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  static const _voicebanks = ['Momo English', 'Teto English', 'Hana JP'];

  late final ApiClient _client = widget.client ?? ApiClient();

  // Engine connection
  late final TextEditingController _serverUrlController = TextEditingController(
    text: _client.baseUrl,
  );
  bool _testingConnection = false;
  bool? _connectionOk;
  String _connectionResult = '';

  // Render Engine
  String _engine = 'WORLD realtime';

  // Audio
  String _outputApi = 'AAudio';
  int _sampleRate = 44100;
  double _bufferSize = 512; // samples, 128..2048

  // Voicebank
  String _defaultVoicebank = 'Momo English';

  // Appearance
  bool _showPitchCurve = true;

  void _snack(String message) {
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(
        SnackBar(
          content: Text(message),
          behavior: SnackBarBehavior.floating,
          backgroundColor: LiltColors.panel2,
        ),
      );
  }

  /// `GET /health` against the entered URL; shows version + `.so` status.
  Future<void> _testConnection() async {
    setState(() {
      _testingConnection = true;
      _connectionOk = null;
      _connectionResult = '';
    });
    try {
      final health = await _client.health();
      if (!mounted) return;
      setState(() {
        _connectionOk = true;
        _connectionResult = health.status == 'ok'
            ? 'Connected · engine v${health.version} · '
                  '${health.soLoaded ? 'renderer loaded' : 'renderer NOT loaded'}'
            : 'Engine reported status "${health.status}"';
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _connectionOk = false;
        _connectionResult = 'Cannot reach engine: $e';
      });
    } finally {
      if (mounted) setState(() => _testingConnection = false);
    }
  }

  @override
  void dispose() {
    _serverUrlController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Settings')),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          _buildConnectionSection(),
          _buildEngineSection(),
          _buildAudioSection(),
          _buildVoicebankSection(),
          _buildAppearanceSection(),
          _buildAboutSection(),
        ],
      ),
    );
  }

  // ---------------------------------------------------------------- sections

  /// Engine connection: server URL + "Test connection" (CONTRACT-v1 §4).
  ///
  /// In-memory only for v1 — the URL lives on the shared [ApiClient], so
  /// Voicebanks and Editor pick it up without a restart.
  Widget _buildConnectionSection() {
    final statusColor = _connectionOk == null
        ? LiltColors.muted
        : (_connectionOk! ? LiltColors.green : _errorRed);
    final statusIcon = _connectionOk == null
        ? Icons.lan_outlined
        : (_connectionOk! ? Icons.cloud_done_rounded : Icons.cloud_off_rounded);
    return _section('Engine connection', [
      Padding(
        padding: const EdgeInsets.fromLTRB(16, 14, 16, 4),
        child: Row(
          children: [
            const Icon(Icons.dns_rounded, size: 20, color: LiltColors.muted),
            const SizedBox(width: 12),
            const Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Server URL',
                    style: TextStyle(color: LiltColors.text, fontSize: 14),
                  ),
                  SizedBox(height: 2),
                  Text(
                    'synth-server (CONTRACT-v1) — edit and tap Test connection.',
                    style: TextStyle(color: LiltColors.muted, fontSize: 12),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
      Padding(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 6),
        child: TextField(
          controller: _serverUrlController,
          onChanged: (value) => _client.baseUrl = value,
          keyboardType: TextInputType.url,
          style: const TextStyle(color: LiltColors.text, fontSize: 13),
          decoration: InputDecoration(
            isDense: true,
            hintText: kDefaultEngineBaseUrl,
            hintStyle: const TextStyle(color: LiltColors.muted, fontSize: 13),
            filled: true,
            fillColor: LiltColors.bg,
            contentPadding: const EdgeInsets.symmetric(
              horizontal: 12,
              vertical: 10,
            ),
            border: OutlineInputBorder(
              borderRadius: BorderRadius.circular(8),
              borderSide: const BorderSide(color: LiltColors.line),
            ),
            enabledBorder: OutlineInputBorder(
              borderRadius: BorderRadius.circular(8),
              borderSide: const BorderSide(color: LiltColors.line),
            ),
            focusedBorder: OutlineInputBorder(
              borderRadius: BorderRadius.circular(8),
              borderSide: const BorderSide(color: LiltColors.purple),
            ),
          ),
        ),
      ),
      Padding(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 4),
        child: Row(
          children: [
            FilledButton.tonalIcon(
              onPressed: _testingConnection ? null : _testConnection,
              icon: _testingConnection
                  ? const SizedBox(
                      width: 14,
                      height: 14,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Icon(Icons.wifi_tethering_rounded, size: 18),
              label: Text(_testingConnection ? 'Testing…' : 'Test connection'),
              style: FilledButton.styleFrom(
                padding: const EdgeInsets.symmetric(horizontal: 14),
                visualDensity: VisualDensity.compact,
                textStyle: const TextStyle(fontSize: 13),
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: _connectionResult.isEmpty
                  ? const Text(
                      'Not tested yet',
                      style: TextStyle(color: LiltColors.muted, fontSize: 12),
                    )
                  : Row(
                      children: [
                        Icon(statusIcon, size: 16, color: statusColor),
                        const SizedBox(width: 6),
                        Expanded(
                          child: Text(
                            _connectionResult,
                            style: TextStyle(
                              color: statusColor,
                              fontSize: 12,
                              fontWeight: FontWeight.w600,
                            ),
                          ),
                        ),
                      ],
                    ),
            ),
          ],
        ),
      ),
      const SizedBox(height: 10),
    ]);
  }

  static const Color _errorRed = Color(0xFFFF6B6B);

  Widget _buildEngineSection() {
    return _section('Render Engine', [
      Padding(
        padding: const EdgeInsets.fromLTRB(16, 14, 16, 4),
        child: Row(
          children: [
            const Icon(Icons.memory_rounded, size: 20, color: LiltColors.muted),
            const SizedBox(width: 12),
            const Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Renderer',
                    style: TextStyle(color: LiltColors.text, fontSize: 14),
                  ),
                  SizedBox(height: 2),
                  Text(
                    'WORLD is the main renderer, Classic is the legacy resampler.',
                    style: TextStyle(color: LiltColors.muted, fontSize: 12),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
      Padding(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 14),
        child: FittedBox(
          fit: BoxFit.scaleDown,
          alignment: Alignment.centerLeft,
          child: SegmentedButton<String>(
            segments: const [
              ButtonSegment(
                value: 'WORLD realtime',
                label: Text('WORLD realtime'),
              ),
              ButtonSegment(value: 'Classic', label: Text('Classic')),
            ],
            selected: {_engine},
            showSelectedIcon: false,
            onSelectionChanged: (selection) {
              setState(() => _engine = selection.first);
              _snack('Engine preference saved (mock)');
            },
          ),
        ),
      ),
    ]);
  }

  Widget _buildAudioSection() {
    return _section('Audio', [
      _row(
        icon: Icons.speaker_rounded,
        title: 'Output API',
        trailing: _dropdown<String>(
          value: _outputApi,
          items: const ['AAudio', 'AudioTrack'],
          onChanged: (v) => setState(() => _outputApi = v!),
        ),
      ),
      _row(
        icon: Icons.av_timer_rounded,
        title: 'Sample rate',
        trailing: _dropdown<int>(
          value: _sampleRate,
          items: const [44100, 48000],
          label: (v) => '$v Hz',
          onChanged: (v) => setState(() => _sampleRate = v!),
        ),
      ),
      _bufferRow(),
    ]);
  }

  Widget _buildVoicebankSection() {
    return _section('Voicebank', [
      _row(
        icon: Icons.piano_rounded,
        title: 'Default voicebank',
        trailing: _dropdown<String>(
          value: _defaultVoicebank,
          items: _voicebanks,
          onChanged: (v) => setState(() => _defaultVoicebank = v!),
        ),
      ),
      _row(
        icon: Icons.storage_rounded,
        title: 'Voicebanks: 214 MB',
        trailing: FilledButton.tonal(
          onPressed: () => _snack('Cache cleared (mock)'),
          style: FilledButton.styleFrom(
            padding: const EdgeInsets.symmetric(horizontal: 12),
            visualDensity: VisualDensity.compact,
            textStyle: const TextStyle(fontSize: 13),
          ),
          child: const Text('Clear cache'),
        ),
      ),
    ]);
  }

  Widget _buildAppearanceSection() {
    return _section('Appearance', [
      _row(
        icon: Icons.dark_mode_rounded,
        title: 'Theme',
        subtitle: 'Dark only — light theme coming later',
        trailing: DropdownButton<String>(
          value: 'Dark',
          onChanged: null, // disabled: app is dark-only for now
          isDense: true,
          underline: const SizedBox.shrink(),
          disabledHint: const Text(
            'Dark',
            style: TextStyle(color: LiltColors.muted, fontSize: 14),
          ),
          items: const [DropdownMenuItem(value: 'Dark', child: Text('Dark'))],
        ),
      ),
      _row(
        icon: Icons.show_chart_rounded,
        title: 'Show pitch curve by default',
        trailing: Switch(
          value: _showPitchCurve,
          activeTrackColor: LiltColors.purple2,
          onChanged: (v) => setState(() => _showPitchCurve = v),
        ),
      ),
    ]);
  }

  Widget _buildAboutSection() {
    return _section('About', [
      _row(
        icon: Icons.info_outline_rounded,
        title: 'Version',
        trailing: const Text(
          'Lilt 0.1.0',
          style: TextStyle(color: LiltColors.muted, fontSize: 14),
        ),
      ),
      _row(
        icon: Icons.link_rounded,
        title: 'OpenUtau compatibility: voicebank/.ustx',
        onTap: () => _snack('OpenUtau compatibility info (mock)'),
        trailing: const Icon(
          Icons.open_in_new_rounded,
          size: 16,
          color: LiltColors.purple,
        ),
      ),
      _row(
        icon: Icons.description_outlined,
        title: 'License',
        trailing: const Text(
          'MIT (based on OpenUtau)',
          style: TextStyle(color: LiltColors.muted, fontSize: 14),
        ),
      ),
    ]);
  }

  // ----------------------------------------------------------------- helpers

  /// Card with a small-caps section title and divider-separated rows.
  Widget _section(String title, List<Widget> rows) {
    return Card(
      margin: const EdgeInsets.only(bottom: 16),
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 14, 16, 8),
            child: Text(
              title.toUpperCase(),
              style: const TextStyle(
                color: LiltColors.muted,
                fontSize: 11,
                fontWeight: FontWeight.w700,
                letterSpacing: 1.1,
              ),
            ),
          ),
          for (var i = 0; i < rows.length; i++) ...[
            if (i > 0)
              const Divider(
                height: 1,
                thickness: 1,
                color: LiltColors.line,
                indent: 48,
              ),
            rows[i],
          ],
        ],
      ),
    );
  }

  /// ListTile-style row: leading icon, title (+optional subtitle), trailing.
  Widget _row({
    required IconData icon,
    required String title,
    String? subtitle,
    Widget? trailing,
    VoidCallback? onTap,
  }) {
    final content = Row(
      children: [
        Icon(icon, size: 20, color: LiltColors.muted),
        const SizedBox(width: 12),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                title,
                style: const TextStyle(color: LiltColors.text, fontSize: 14),
              ),
              if (subtitle != null) ...[
                const SizedBox(height: 2),
                Text(
                  subtitle,
                  style: const TextStyle(color: LiltColors.muted, fontSize: 12),
                ),
              ],
            ],
          ),
        ),
        if (trailing != null) ...[
          const SizedBox(width: 12),
          // Keep trailing bounded so long labels/dropdowns never overflow
          // on narrow phones: scale it down instead of overflowing.
          ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 170),
            child: FittedBox(
              fit: BoxFit.scaleDown,
              alignment: Alignment.centerRight,
              child: trailing,
            ),
          ),
        ],
      ],
    );
    final padding = const EdgeInsets.symmetric(horizontal: 16, vertical: 12);
    if (onTap == null) {
      return Padding(padding: padding, child: content);
    }
    return InkWell(
      onTap: onTap,
      child: Padding(padding: padding, child: content),
    );
  }

  /// Compact dark DropdownButton used as a row trailing control.
  Widget _dropdown<T>({
    required T value,
    required List<T> items,
    required ValueChanged<T?> onChanged,
    String Function(T)? label,
  }) {
    return DropdownButton<T>(
      value: value,
      isDense: true,
      underline: const SizedBox.shrink(),
      borderRadius: BorderRadius.circular(8),
      dropdownColor: LiltColors.panel2,
      icon: const Icon(Icons.arrow_drop_down_rounded, color: LiltColors.muted),
      style: const TextStyle(color: LiltColors.text, fontSize: 14),
      onChanged: onChanged,
      items: [
        for (final item in items)
          DropdownMenuItem(
            value: item,
            child: Text(label?.call(item) ?? '$item'),
          ),
      ],
    );
  }

  /// Buffer-size row: label + current value, slider below.
  Widget _bufferRow() {
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 10, 16, 6),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Icon(
                Icons.data_usage_rounded,
                size: 20,
                color: LiltColors.muted,
              ),
              const SizedBox(width: 12),
              const Expanded(
                child: Text(
                  'Buffer size',
                  style: TextStyle(color: LiltColors.text, fontSize: 14),
                ),
              ),
              Text(
                '${_bufferSize.round()} samples',
                style: const TextStyle(color: LiltColors.purple, fontSize: 13),
              ),
            ],
          ),
          Slider(
            value: _bufferSize,
            min: 128,
            max: 2048,
            divisions: 120, // 16-sample steps
            label: '${_bufferSize.round()} samples',
            activeColor: LiltColors.purple,
            inactiveColor: LiltColors.line,
            onChanged: (v) => setState(() => _bufferSize = v),
          ),
        ],
      ),
    );
  }
}
