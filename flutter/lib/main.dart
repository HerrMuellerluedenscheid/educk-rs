import 'package:flutter/material.dart';
import 'screens/home_screen.dart';

void main() => runApp(const EnergyDashboardApp());

class EnergyDashboardApp extends StatelessWidget {
  const EnergyDashboardApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Energy Dashboard',
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
