import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:energy_dashboard/main.dart';

void main() {
  testWidgets('app boots to the loading state', (WidgetTester tester) async {
    await tester.pumpWidget(const EnergyDashboardApp());

    // The dashboard fetches over the network on start; in tests the mocked
    // HTTP client keeps the future pending/failing, so the first frame must
    // show the spinner rather than crash.
    expect(find.byType(CircularProgressIndicator), findsOneWidget);
  });
}
