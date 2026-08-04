/// Lilt dark theme — colors lifted from the ui-mock HTML mock.
library;

import 'package:flutter/material.dart';

/// Palette (kept in sync with ui-mock/index.html).
abstract final class LiltColors {
  static const bg = Color(0xFF111217);
  static const panel = Color(0xFF181A22);
  static const panel2 = Color(0xFF20232D);
  static const line = Color(0xFF303441);
  static const muted = Color(0xFF8E94A7);
  static const text = Color(0xFFEEF0F7);

  static const purple = Color(0xFFA98CFF);
  static const purple2 = Color(0xFF7358E9);
  static const cyan = Color(0xFF55D7DE);
  static const pink = Color(0xFFFF83B8);
  static const green = Color(0xFF7BE3A8);
  static const orange = Color(0xFFFFC16D);

  static const trackColors = [purple, cyan, pink];
}

ThemeData buildLiltTheme() {
  final base = ThemeData.dark(useMaterial3: true);
  return base.copyWith(
    scaffoldBackgroundColor: LiltColors.bg,
    colorScheme: base.colorScheme.copyWith(
      primary: LiltColors.purple,
      secondary: LiltColors.cyan,
      surface: LiltColors.panel,
      onSurface: LiltColors.text,
      outline: LiltColors.line,
    ),
    dividerColor: LiltColors.line,
    navigationRailTheme: NavigationRailThemeData(
      backgroundColor: const Color(0xFF15161D),
      indicatorColor: const Color(0xFF292536),
      selectedIconTheme: const IconThemeData(color: LiltColors.purple),
      selectedLabelTextStyle: const TextStyle(
        color: LiltColors.text,
        fontSize: 11,
      ),
      unselectedIconTheme: const IconThemeData(color: Color(0xFF777D91)),
      unselectedLabelTextStyle: const TextStyle(
        color: Color(0xFF777D91),
        fontSize: 11,
      ),
    ),
    appBarTheme: const AppBarTheme(
      backgroundColor: Color(0xFF17181F),
      foregroundColor: LiltColors.text,
      elevation: 0,
      scrolledUnderElevation: 0,
    ),
    cardTheme: CardThemeData(
      color: LiltColors.panel2,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(10),
        side: const BorderSide(color: LiltColors.line),
      ),
    ),
    chipTheme: base.chipTheme.copyWith(
      backgroundColor: LiltColors.panel2,
      side: const BorderSide(color: Color(0xFF393D4D)),
      labelStyle: const TextStyle(color: Color(0xFFAEB3C2), fontSize: 11),
    ),
  );
}
