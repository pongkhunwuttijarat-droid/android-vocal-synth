import 'package:flutter/material.dart';

import 'api/engine_client.dart';
import 'screens/editor_screen.dart';
import 'screens/settings_screen.dart';
import 'screens/voicebanks_screen.dart';
import 'theme.dart';

void main() {
  runApp(const LiltApp());
}

class LiltApp extends StatelessWidget {
  const LiltApp({super.key, this.client});

  /// Engine client override — tests inject a mocked client; the app uses
  /// a real one (default) that Settings can re-point at another server.
  final ApiClient? client;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Lilt',
      debugShowCheckedModeBanner: false,
      theme: buildLiltTheme(),
      home: AppShell(client: client),
    );
  }
}

/// Root shell: rail navigation (editor / voicebanks / settings).
///
/// Desktop/tablet: [NavigationRail] on the left, matching the ui-mock.
/// Narrow phones: bottom [NavigationBar].
class AppShell extends StatefulWidget {
  const AppShell({super.key, this.client});

  final ApiClient? client;

  @override
  State<AppShell> createState() => _AppShellState();
}

class _AppShellState extends State<AppShell> {
  int _index = 0;

  late final ApiClient _client = widget.client ?? ApiClient();

  late final List<Widget> _screens = [
    EditorScreen(client: _client),
    VoicebanksScreen(client: _client),
    SettingsScreen(client: _client),
  ];

  @override
  Widget build(BuildContext context) {
    final rail = NavigationRail(
      selectedIndex: _index,
      onDestinationSelected: (i) => setState(() => _index = i),
      labelType: NavigationRailLabelType.all,
      destinations: const [
        NavigationRailDestination(
          icon: Icon(Icons.grid_view_rounded),
          label: Text('Editor'),
        ),
        NavigationRailDestination(
          icon: Icon(Icons.piano_rounded),
          label: Text('Voicebanks'),
        ),
        NavigationRailDestination(
          icon: Icon(Icons.settings_rounded),
          label: Text('Settings'),
        ),
      ],
    );

    return LayoutBuilder(
      builder: (context, constraints) {
        final wide = constraints.maxWidth >= 720;
        return Scaffold(
          body: Row(
            children: [
              if (wide) rail,
              Expanded(
                child: IndexedStack(index: _index, children: _screens),
              ),
            ],
          ),
          bottomNavigationBar: wide
              ? null
              : NavigationBar(
                  selectedIndex: _index,
                  onDestinationSelected: (i) => setState(() => _index = i),
                  destinations: const [
                    NavigationDestination(
                      icon: Icon(Icons.grid_view_rounded),
                      label: 'Editor',
                    ),
                    NavigationDestination(
                      icon: Icon(Icons.piano_rounded),
                      label: 'Voicebanks',
                    ),
                    NavigationDestination(
                      icon: Icon(Icons.settings_rounded),
                      label: 'Settings',
                    ),
                  ],
                ),
        );
      },
    );
  }
}
