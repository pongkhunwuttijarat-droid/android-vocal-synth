import 'package:flutter/material.dart';

import '../api/engine_client.dart';
import '../models.dart';
import '../theme.dart';

/// Error red — not in the Lilt palette; local to the voicebank screen.
const _errorRed = Color(0xFFFF6B6B);

/// Mock banks shown when the engine server is unreachable (CONTRACT-v1 §4:
/// UI options must NEVER disappear). Keep the same spread of statuses so
/// every card state stays visible offline.
const List<Voicebank> _mockBanks = [
  Voicebank(
    name: 'Momo English',
    format: 'OpenUtau',
    singer: 'English CVVC',
    status: 'ready',
    sizeMb: 412,
  ),
  Voicebank(
    name: 'Teto English',
    format: 'OpenUtau',
    singer: 'English VCCV',
    status: 'ready',
    sizeMb: 388,
  ),
  Voicebank(
    name: 'Hana JP',
    format: 'UTAU',
    singer: 'Japanese CV',
    status: 'importing',
    sizeMb: 96,
  ),
  Voicebank(
    name: 'Old Bank 01',
    format: 'OpenUtau',
    singer: 'Unknown voice',
    status: 'error',
    sizeMb: 260,
  ),
];

/// Voicebank manager — separate page per the UI decision (not in the editor).
///
/// v1: the list loads live from `GET /voicebanks` (via [ApiClient]). If the
/// engine server is unreachable the screen falls back to [_mockBanks] with
/// an "offline" banner + retry — the import / detail / status UI never
/// disappears. SAF import is a later milestone, so the Import button only
/// shows a placeholder SnackBar for now.
class VoicebanksScreen extends StatefulWidget {
  const VoicebanksScreen({super.key, this.client});

  /// Engine client; defaults to a real one when not injected (tests inject
  /// a mocked client).
  final ApiClient? client;

  @override
  State<VoicebanksScreen> createState() => _VoicebanksScreenState();
}

class _VoicebanksScreenState extends State<VoicebanksScreen> {
  late final ApiClient _client = widget.client ?? ApiClient();

  List<Voicebank> _banks = const [];
  bool _loading = true;
  bool _offline = false;
  String? _offlineReason;

  String? _defaultName;

  @override
  void initState() {
    super.initState();
    _load();
  }

  /// Fetch the live bank list; on ANY failure fall back to the mock list
  /// plus the offline banner (never hide the options).
  Future<void> _load() async {
    setState(() {
      _loading = true;
      _offline = false;
      _offlineReason = null;
    });
    try {
      final banks = await _client.voicebanks();
      if (!mounted) return;
      setState(() {
        _banks = banks;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _banks = _mockBanks;
        _offline = true;
        _offlineReason = e.toString();
        _loading = false;
      });
    }
  }

  void _snack(String message) {
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(content: Text(message)));
  }

  void _onImport() => _snack('Import via SAF coming soon');

  void _retry(Voicebank bank) {
    final i = _banks.indexOf(bank);
    setState(() {
      _banks[i] = Voicebank(
        name: bank.name,
        format: bank.format,
        singer: bank.singer,
        status: 'ready',
        sizeMb: bank.sizeMb,
      );
    });
    _snack('${bank.name} re-imported');
  }

  void _delete(Voicebank bank) {
    setState(() => _banks.removeWhere((b) => b.name == bank.name));
    if (_defaultName == bank.name) _defaultName = null;
    _snack('${bank.name} deleted');
  }

  void _setDefault(Voicebank bank) {
    setState(() => _defaultName = bank.name);
    _snack('${bank.name} set as default');
  }

  Future<void> _showDetailSheet(Voicebank bank) {
    return showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      backgroundColor: LiltColors.panel,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(16)),
        side: BorderSide(color: LiltColors.line),
      ),
      builder: (sheetContext) => _BankDetailSheet(
        bank: bank,
        isDefault: bank.name == _defaultName,
        onSetDefault: () {
          Navigator.pop(sheetContext);
          _setDefault(bank);
        },
        onDelete: () {
          Navigator.pop(sheetContext);
          _delete(bank);
        },
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Voicebanks'),
        actions: [
          IconButton(
            icon: const Icon(Icons.refresh_rounded),
            tooltip: 'Reload from engine',
            onPressed: _loading ? null : _load,
          ),
          Padding(
            padding: const EdgeInsets.only(right: 12),
            child: FilledButton.icon(
              onPressed: _onImport,
              icon: const Icon(Icons.file_open, size: 18),
              label: const Text('Import'),
              style: FilledButton.styleFrom(
                backgroundColor: LiltColors.purple2,
                foregroundColor: Colors.white,
                visualDensity: VisualDensity.compact,
              ),
            ),
          ),
        ],
      ),
      body: Column(
        children: [
          if (_offline) ...[
            _OfflineBanner(reason: _offlineReason, onRetry: _load),
            const SizedBox(height: 4),
          ],
          _SummaryHeader(banks: _banks),
          Expanded(
            child: _loading
                ? const Center(child: CircularProgressIndicator())
                : _banks.isEmpty
                ? const _EmptyState()
                : ListView.separated(
                    padding: const EdgeInsets.fromLTRB(16, 12, 16, 24),
                    itemCount: _banks.length,
                    separatorBuilder: (_, _) => const SizedBox(height: 10),
                    itemBuilder: (context, i) => _buildBankCard(_banks[i]),
                  ),
          ),
        ],
      ),
    );
  }

  Widget _buildBankCard(Voicebank bank) {
    return Card(
      margin: EdgeInsets.zero,
      child: InkWell(
        borderRadius: BorderRadius.circular(10),
        onTap: () => _showDetailSheet(bank),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(14, 14, 10, 12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  _BankAvatar(bank: bank),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          children: [
                            Flexible(
                              child: Text(
                                bank.name,
                                overflow: TextOverflow.ellipsis,
                                style: const TextStyle(
                                  color: LiltColors.text,
                                  fontSize: 15,
                                  fontWeight: FontWeight.w700,
                                ),
                              ),
                            ),
                            if (bank.name == _defaultName) ...[
                              const SizedBox(width: 6),
                              const _DefaultTag(),
                            ],
                          ],
                        ),
                        const SizedBox(height: 2),
                        Text(
                          '${bank.format} · ${bank.singer}',
                          overflow: TextOverflow.ellipsis,
                          style: const TextStyle(
                            color: LiltColors.muted,
                            fontSize: 12,
                          ),
                        ),
                      ],
                    ),
                  ),
                  const SizedBox(width: 8),
                  _StatusBadge(status: bank.status),
                  if (bank.status == 'error')
                    IconButton(
                      onPressed: () => _retry(bank),
                      tooltip: 'Retry',
                      icon: const Icon(Icons.refresh_rounded, size: 18),
                      color: _errorRed,
                      visualDensity: VisualDensity.compact,
                    ),
                ],
              ),
              const SizedBox(height: 10),
              Row(
                children: [
                  const Icon(
                    Icons.sd_storage_outlined,
                    size: 14,
                    color: LiltColors.muted,
                  ),
                  const SizedBox(width: 5),
                  Text(
                    _formatMb(bank.sizeMb),
                    style: const TextStyle(
                      color: LiltColors.muted,
                      fontSize: 12,
                    ),
                  ),
                  const Spacer(),
                  if (bank.status == 'importing')
                    const Text(
                      'Importing…',
                      style: TextStyle(color: LiltColors.orange, fontSize: 11),
                    ),
                ],
              ),
              if (bank.status == 'importing') ...[
                const SizedBox(height: 10),
                ClipRRect(
                  borderRadius: BorderRadius.circular(3),
                  child: const LinearProgressIndicator(
                    minHeight: 4,
                    backgroundColor: LiltColors.line,
                    color: LiltColors.orange,
                  ),
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }
}

Color _statusColor(String status) => switch (status) {
  'ready' => LiltColors.green,
  'importing' => LiltColors.orange,
  'error' => _errorRed,
  _ => LiltColors.muted,
};

String _formatMb(int mb) {
  if (mb >= 1024) return '${(mb / 1024).toStringAsFixed(1)} GB';
  return '$mb MB';
}

/// Summary strip: ready/importing/error counts + total installed size.
class _SummaryHeader extends StatelessWidget {
  const _SummaryHeader({required this.banks});

  final List<Voicebank> banks;

  @override
  Widget build(BuildContext context) {
    final ready = banks.where((b) => b.status == 'ready').length;
    final importing = banks.where((b) => b.status == 'importing').length;
    final error = banks.where((b) => b.status == 'error').length;
    final totalMb = banks.fold<int>(0, (sum, b) => sum + b.sizeMb);

    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 12, 16, 0),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
        decoration: BoxDecoration(
          color: LiltColors.panel,
          borderRadius: BorderRadius.circular(10),
          border: Border.all(color: LiltColors.line),
        ),
        child: Row(
          children: [
            _CountDot(color: LiltColors.green, label: '$ready ready'),
            if (importing > 0) ...[
              const SizedBox(width: 14),
              _CountDot(
                color: LiltColors.orange,
                label: '$importing importing',
              ),
            ],
            if (error > 0) ...[
              const SizedBox(width: 14),
              _CountDot(color: _errorRed, label: '$error error'),
            ],
            const Spacer(),
            const Icon(
              Icons.storage_rounded,
              size: 14,
              color: LiltColors.muted,
            ),
            const SizedBox(width: 4),
            Text(
              '${_formatMb(totalMb)} total',
              style: const TextStyle(color: LiltColors.muted, fontSize: 12),
            ),
          ],
        ),
      ),
    );
  }
}

class _CountDot extends StatelessWidget {
  const _CountDot({required this.color, required this.label});

  final Color color;
  final String label;

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Container(
          width: 7,
          height: 7,
          decoration: BoxDecoration(color: color, shape: BoxShape.circle),
        ),
        const SizedBox(width: 6),
        Text(
          label,
          style: const TextStyle(color: LiltColors.muted, fontSize: 12),
        ),
      ],
    );
  }
}

/// Rounded-square avatar with a status-tinted gradient + first letter.
class _BankAvatar extends StatelessWidget {
  const _BankAvatar({required this.bank});

  final Voicebank bank;

  @override
  Widget build(BuildContext context) {
    final (begin, end) = switch (bank.status) {
      'importing' => (LiltColors.orange, LiltColors.pink),
      'error' => (_errorRed, LiltColors.orange),
      _ => (LiltColors.purple2, LiltColors.purple),
    };
    return Container(
      width: 40,
      height: 40,
      alignment: Alignment.center,
      decoration: BoxDecoration(
        gradient: LinearGradient(
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
          colors: [begin, end],
        ),
        borderRadius: BorderRadius.circular(12),
      ),
      child: Text(
        bank.name.substring(0, 1).toUpperCase(),
        style: const TextStyle(
          color: Colors.white,
          fontSize: 16,
          fontWeight: FontWeight.w800,
        ),
      ),
    );
  }
}

class _StatusBadge extends StatelessWidget {
  const _StatusBadge({required this.status});

  final String status;

  @override
  Widget build(BuildContext context) {
    final color = _statusColor(status);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.12),
        borderRadius: BorderRadius.circular(999),
        border: Border.all(color: color.withValues(alpha: 0.4)),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Container(
            width: 6,
            height: 6,
            decoration: BoxDecoration(color: color, shape: BoxShape.circle),
          ),
          const SizedBox(width: 5),
          Text(
            status.toUpperCase(),
            style: TextStyle(
              color: color,
              fontSize: 10,
              fontWeight: FontWeight.w700,
              letterSpacing: 0.8,
            ),
          ),
        ],
      ),
    );
  }
}

class _DefaultTag extends StatelessWidget {
  const _DefaultTag();

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
      decoration: BoxDecoration(
        color: LiltColors.purple.withValues(alpha: 0.15),
        borderRadius: BorderRadius.circular(6),
        border: Border.all(color: LiltColors.purple.withValues(alpha: 0.5)),
      ),
      child: const Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(Icons.star_rounded, size: 11, color: LiltColors.purple),
          SizedBox(width: 3),
          Text(
            'DEFAULT',
            style: TextStyle(
              color: LiltColors.purple,
              fontSize: 9,
              fontWeight: FontWeight.w700,
              letterSpacing: 0.6,
            ),
          ),
        ],
      ),
    );
  }
}

/// Bottom sheet: full bank info + set-as-default / delete actions.
class _BankDetailSheet extends StatelessWidget {
  const _BankDetailSheet({
    required this.bank,
    required this.isDefault,
    required this.onSetDefault,
    required this.onDelete,
  });

  final Voicebank bank;
  final bool isDefault;
  final VoidCallback onSetDefault;
  final VoidCallback onDelete;

  @override
  Widget build(BuildContext context) {
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(20, 8, 20, 20),
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Center(
                child: Container(
                  width: 36,
                  height: 4,
                  decoration: BoxDecoration(
                    color: LiltColors.line,
                    borderRadius: BorderRadius.circular(2),
                  ),
                ),
              ),
              const SizedBox(height: 16),
              Row(
                children: [
                  _BankAvatar(bank: bank),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          children: [
                            Flexible(
                              child: Text(
                                bank.name,
                                overflow: TextOverflow.ellipsis,
                                style: const TextStyle(
                                  color: LiltColors.text,
                                  fontSize: 16,
                                  fontWeight: FontWeight.w700,
                                ),
                              ),
                            ),
                            if (isDefault) ...[
                              const SizedBox(width: 6),
                              const _DefaultTag(),
                            ],
                          ],
                        ),
                        const SizedBox(height: 2),
                        Text(
                          '${bank.format} · ${bank.singer}',
                          style: const TextStyle(
                            color: LiltColors.muted,
                            fontSize: 12,
                          ),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 16),
              _InfoRow(label: 'Format', value: bank.format),
              _InfoRow(label: 'Singer', value: bank.singer),
              _InfoRow(label: 'Size', value: _formatMb(bank.sizeMb)),
              _InfoRow(
                label: 'Status',
                value: bank.status.toUpperCase(),
                valueColor: _statusColor(bank.status),
              ),
              const SizedBox(height: 20),
              SizedBox(
                width: double.infinity,
                child: FilledButton.icon(
                  onPressed: isDefault ? null : onSetDefault,
                  icon: const Icon(Icons.star_rounded, size: 18),
                  label: Text(
                    isDefault ? 'Default voicebank' : 'Set as default',
                  ),
                ),
              ),
              const SizedBox(height: 8),
              SizedBox(
                width: double.infinity,
                child: OutlinedButton.icon(
                  onPressed: onDelete,
                  style: OutlinedButton.styleFrom(
                    foregroundColor: _errorRed,
                    side: const BorderSide(color: _errorRed),
                  ),
                  icon: const Icon(Icons.delete_outline_rounded, size: 18),
                  label: const Text('Delete'),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _InfoRow extends StatelessWidget {
  const _InfoRow({required this.label, required this.value, this.valueColor});

  final String label;
  final String value;
  final Color? valueColor;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 5),
      child: Row(
        children: [
          SizedBox(
            width: 64,
            child: Text(
              label,
              style: const TextStyle(color: LiltColors.muted, fontSize: 12),
            ),
          ),
          Expanded(
            child: Text(
              value,
              textAlign: TextAlign.right,
              style: TextStyle(
                color: valueColor ?? LiltColors.text,
                fontSize: 12,
                fontWeight: FontWeight.w600,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _EmptyState extends StatelessWidget {
  const _EmptyState();

  @override
  Widget build(BuildContext context) {
    return const Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(Icons.library_music_outlined, size: 42, color: LiltColors.muted),
          SizedBox(height: 10),
          Text(
            'No voicebanks installed',
            style: TextStyle(
              color: LiltColors.text,
              fontWeight: FontWeight.w600,
            ),
          ),
          SizedBox(height: 4),
          Text(
            'Tap Import to add one',
            style: TextStyle(color: LiltColors.muted, fontSize: 12),
          ),
        ],
      ),
    );
  }
}

/// Warning strip shown when the engine server is unreachable: the mock
/// list is on screen, and this banner explains why + offers a retry.
class _OfflineBanner extends StatelessWidget {
  const _OfflineBanner({required this.reason, required this.onRetry});

  final String? reason;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) {
    return Container(
      margin: const EdgeInsets.fromLTRB(16, 12, 16, 0),
      padding: const EdgeInsets.fromLTRB(12, 10, 8, 10),
      decoration: BoxDecoration(
        color: LiltColors.orange.withValues(alpha: 0.10),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: LiltColors.orange.withValues(alpha: 0.4)),
      ),
      child: Row(
        children: [
          const Icon(
            Icons.cloud_off_rounded,
            size: 18,
            color: LiltColors.orange,
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Text(
                  'Offline — showing mock data',
                  style: TextStyle(
                    color: LiltColors.text,
                    fontSize: 13,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                if (reason != null && reason!.isNotEmpty) ...[
                  const SizedBox(height: 2),
                  Text(
                    reason!,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      color: LiltColors.muted,
                      fontSize: 11,
                    ),
                  ),
                ],
              ],
            ),
          ),
          const SizedBox(width: 8),
          TextButton(
            onPressed: onRetry,
            style: TextButton.styleFrom(
              foregroundColor: LiltColors.orange,
              visualDensity: VisualDensity.compact,
            ),
            child: const Text('Retry'),
          ),
        ],
      ),
    );
  }
}
