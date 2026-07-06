import 'package:flutter/material.dart';
import 'package:intl/date_symbol_data_local.dart';
import 'package:intl/intl.dart';
import 'i18n.dart';
import 'screens/home_screen.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  // Language follows the device locale (EN/DE), like the web dashboard.
  L10n.init();
  Intl.defaultLocale = L10n.current.code;
  await initializeDateFormatting(L10n.current.code);
  runApp(const EnergyDashboardApp());
}

class EnergyDashboardApp extends StatelessWidget {
  const EnergyDashboardApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'educk',
      debugShowCheckedModeBanner: false,
      theme: _theme(Brightness.light),
      darkTheme: _theme(Brightness.dark),
      home: const HomeScreen(),
    );
  }

  ThemeData _theme(Brightness brightness) {
    return ThemeData(
      useMaterial3: true,
      colorScheme: ColorScheme.fromSeed(
        seedColor: const Color(0xFF2E7D32), // forest green
        brightness: brightness,
      ),
    );
  }
}
