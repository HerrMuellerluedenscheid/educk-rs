import 'dart:math';
import 'package:fl_chart/fl_chart.dart';
import 'package:flutter/material.dart';
import 'package:intl/intl.dart';
import '../models/energy_data.dart';

/// Compact 24-hour surplus sparkline — no numbers, just the shape.
class MiniForecast extends StatelessWidget {
  final EnergyData data;

  const MiniForecast({super.key, required this.data});

  @override
  Widget build(BuildContext context) {
    final pts = data.points;
    if (pts.isEmpty) return const SizedBox.shrink();

    final now = DateTime.now();
    double? nowX;
    for (int i = 0; i < pts.length; i++) {
      if (pts[i].timestamp.isAfter(now)) {
        nowX = i.toDouble();
        break;
      }
    }

    final surplusValues = pts.map((p) => p.surplus).toList();
    final lo = surplusValues.fold(double.infinity, min);
    final hi = surplusValues.fold(double.negativeInfinity, max);
    final padding = (hi - lo) * 0.1;
    final minY = lo - padding;
    final maxY = hi + padding;

    final spots = pts
        .asMap()
        .entries
        .map((e) => FlSpot(e.key.toDouble(), e.value.surplus))
        .toList();

    // Show ~4 time labels
    final interval = max(1, (pts.length / 4).ceil()).toDouble();

    return LineChart(
      LineChartData(
        minX: 0,
        maxX: (pts.length - 1).toDouble(),
        minY: minY,
        maxY: maxY,
        clipData: const FlClipData.all(),
        gridData: const FlGridData(show: false),
        borderData: FlBorderData(show: false),
        lineTouchData: const LineTouchData(enabled: false),
        extraLinesData: ExtraLinesData(
          horizontalLines: [
            HorizontalLine(
              y: 0,
              color: Colors.grey.withOpacity(0.3),
              strokeWidth: 1,
              dashArray: [4, 3],
            ),
          ],
          verticalLines: nowX != null
              ? [
                  VerticalLine(
                    x: nowX,
                    color: Colors.orange.shade500,
                    strokeWidth: 1.5,
                    dashArray: [4, 3],
                    label: VerticalLineLabel(
                      show: true,
                      alignment: Alignment.topRight,
                      padding: const EdgeInsets.only(left: 4, bottom: 2),
                      style: TextStyle(
                        fontSize: 9,
                        fontWeight: FontWeight.w600,
                        color: Colors.orange.shade600,
                      ),
                      labelResolver: (_) => 'Now',
                    ),
                  ),
                ]
              : [],
        ),
        titlesData: FlTitlesData(
          rightTitles:
              const AxisTitles(sideTitles: SideTitles(showTitles: false)),
          topTitles:
              const AxisTitles(sideTitles: SideTitles(showTitles: false)),
          leftTitles:
              const AxisTitles(sideTitles: SideTitles(showTitles: false)),
          bottomTitles: AxisTitles(
            sideTitles: SideTitles(
              showTitles: true,
              reservedSize: 22,
              interval: interval,
              getTitlesWidget: (value, meta) {
                final idx = value.toInt();
                if (idx < 0 ||
                    idx >= pts.length ||
                    value == meta.min ||
                    value == meta.max) {
                  return const SizedBox.shrink();
                }
                return Padding(
                  padding: const EdgeInsets.only(top: 4),
                  child: Text(
                    DateFormat('HH:mm').format(pts[idx].timestamp),
                    style: TextStyle(
                      fontSize: 9,
                      color: Colors.grey.withOpacity(0.6),
                    ),
                  ),
                );
              },
            ),
          ),
        ),
        lineBarsData: [
          LineChartBarData(
            spots: spots,
            color: const Color(0xFF43A047),
            barWidth: 2,
            isCurved: true,
            curveSmoothness: 0.3,
            dotData: const FlDotData(show: false),
            belowBarData: BarAreaData(
              show: true,
              gradient: LinearGradient(
                begin: Alignment.topCenter,
                end: Alignment.bottomCenter,
                colors: [
                  const Color(0xFF43A047).withOpacity(0.35),
                  const Color(0xFF43A047).withOpacity(0.05),
                ],
              ),
              cutOffY: 0,
              applyCutOffY: true,
            ),
            aboveBarData: BarAreaData(
              show: true,
              color: Colors.red.shade300.withOpacity(0.18),
              cutOffY: 0,
              applyCutOffY: true,
            ),
          ),
        ],
      ),
    );
  }
}
