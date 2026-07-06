import 'dart:math';
import 'package:fl_chart/fl_chart.dart';
import 'package:flutter/material.dart';
import 'package:intl/intl.dart';
import '../i18n.dart';
import '../models/energy_data.dart';

/// Default chart — the "educk curve": renewable coverage of load (%) over
/// time, with the 100 % ("fully renewable") line emphasised. Mirrors the
/// overview chart on the web dashboard.
class OverviewChart extends StatelessWidget {
  final EnergyData data;

  const OverviewChart({super.key, required this.data});

  static const _curveColor = Color(0xFF059669); // emerald-600, like the web

  @override
  Widget build(BuildContext context) {
    final t = L10n.current;
    final pts = data.points;
    if (pts.isEmpty) return const Center(child: Text('No data'));

    final shares = pts.map((p) => p.renewableShare).toList();
    final maxShare = shares.fold(0.0, max);
    // Headroom, and keep the 100 % line in view even on all-deficit days.
    final maxY = max(100.0, maxShare) * 1.08;

    // X index of the first future point → "Now" marker
    final now = DateTime.now();
    double? nowX;
    for (int i = 0; i < pts.length; i++) {
      if (pts[i].timestamp.isAfter(now)) {
        nowX = i.toDouble();
        break;
      }
    }

    final spots = List.generate(
        pts.length, (i) => FlSpot(i.toDouble(), shares[i]));

    // Show ~8 x-axis labels regardless of data density
    final xInterval = max(1, (pts.length / 8).ceil()).toDouble();

    return LineChart(
      LineChartData(
        minX: 0,
        maxX: (pts.length - 1).toDouble(),
        minY: 0,
        maxY: maxY,
        clipData: const FlClipData.all(),

        // ── Grid: every 25 % ─────────────────────────────────────────────────
        gridData: FlGridData(
          show: true,
          drawVerticalLine: false,
          horizontalInterval: 25,
          getDrawingHorizontalLine: (_) => FlLine(
            color: Colors.grey.withOpacity(0.15),
            strokeWidth: 1,
          ),
        ),
        borderData: FlBorderData(show: false),

        // ── Reference lines ─────────────────────────────────────────────────
        extraLinesData: ExtraLinesData(
          horizontalLines: [
            // 100 % — fully renewable
            HorizontalLine(
              y: 100,
              color: Colors.grey.shade500,
              strokeWidth: 1,
              dashArray: [4, 3],
            ),
          ],
          verticalLines: nowX != null
              ? [
                  VerticalLine(
                    x: nowX,
                    color: Colors.orange.shade600,
                    strokeWidth: 1.5,
                    dashArray: [6, 3],
                    label: VerticalLineLabel(
                      show: true,
                      alignment: Alignment.topRight,
                      padding: const EdgeInsets.only(left: 4),
                      style: TextStyle(
                        color: Colors.orange.shade700,
                        fontSize: 10,
                        fontWeight: FontWeight.w600,
                      ),
                      labelResolver: (_) => t.now,
                    ),
                  ),
                ]
              : [],
        ),

        // ── Axis labels ──────────────────────────────────────────────────────
        titlesData: FlTitlesData(
          rightTitles:
              const AxisTitles(sideTitles: SideTitles(showTitles: false)),
          topTitles:
              const AxisTitles(sideTitles: SideTitles(showTitles: false)),
          leftTitles: AxisTitles(
            sideTitles: SideTitles(
              showTitles: true,
              reservedSize: 40,
              interval: 25,
              getTitlesWidget: (value, meta) {
                if (value == meta.max) return const SizedBox.shrink();
                return Text(
                  '${value.toInt()}%',
                  style: TextStyle(fontSize: 10, color: Colors.grey.shade600),
                  textAlign: TextAlign.right,
                );
              },
            ),
          ),
          bottomTitles: AxisTitles(
            sideTitles: SideTitles(
              showTitles: true,
              reservedSize: 28,
              interval: xInterval,
              getTitlesWidget: (value, meta) {
                final idx = value.toInt();
                if (idx < 0 ||
                    idx >= pts.length ||
                    value == meta.max ||
                    value == meta.min) {
                  return const SizedBox.shrink();
                }
                return Padding(
                  padding: const EdgeInsets.only(top: 4),
                  child: Text(
                    DateFormat('HH:mm').format(pts[idx].timestamp),
                    style:
                        TextStyle(fontSize: 10, color: Colors.grey.shade600),
                  ),
                );
              },
            ),
          ),
        ),

        // ── Touch tooltip ────────────────────────────────────────────────────
        lineTouchData: LineTouchData(
          enabled: true,
          touchTooltipData: LineTouchTooltipData(
            getTooltipColor: (_) => Colors.blueGrey.shade900,
            tooltipRoundedRadius: 8,
            fitInsideHorizontally: true,
            fitInsideVertically: true,
            getTooltipItems: (touched) {
              return touched.map((spot) {
                final idx = spot.x.toInt().clamp(0, pts.length - 1);
                final pt = pts[idx];
                final time = DateFormat('HH:mm').format(pt.timestamp);
                return LineTooltipItem(
                  '$time\n● ${t.renShare}: ${spot.y.toStringAsFixed(0)}%',
                  const TextStyle(
                      color: Color(0xFF34D399),
                      fontSize: 12,
                      fontFamily: 'monospace'),
                );
              }).toList();
            },
          ),
        ),

        // ── The curve ────────────────────────────────────────────────────────
        lineBarsData: [
          LineChartBarData(
            spots: spots,
            color: _curveColor,
            barWidth: 2.5,
            isCurved: false,
            dotData: const FlDotData(show: false),
            belowBarData: BarAreaData(
              show: true,
              gradient: LinearGradient(
                begin: Alignment.topCenter,
                end: Alignment.bottomCenter,
                colors: [
                  _curveColor.withOpacity(0.5),
                  _curveColor.withOpacity(0.04),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}
