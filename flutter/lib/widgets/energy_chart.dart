import 'dart:math';
import 'package:fl_chart/fl_chart.dart';
import 'package:flutter/material.dart';
import 'package:intl/intl.dart';
import '../i18n.dart';
import '../models/energy_data.dart';

class EnergyChart extends StatelessWidget {
  final EnergyData data;

  const EnergyChart({super.key, required this.data});

  // Line colours
  static const _genColor = Color(0xFF2E7D32); // forest green
  static const _loadColor = Color(0xFF1565C0); // strong blue
  static const _surplusColor = Color(0xFFF57C00); // amber-orange

  @override
  Widget build(BuildContext context) {
    final pts = data.points;
    if (pts.isEmpty) return const Center(child: Text('No data'));

    final maxY = data.maxY * 1.15;
    final rawMin = data.minY;
    final minY = rawMin < 0 ? rawMin * 1.2 : -(maxY * 0.08);

    // X index of the first future point → "Now" marker
    final now = DateTime.now();
    double? nowX;
    for (int i = 0; i < pts.length; i++) {
      if (pts[i].timestamp.isAfter(now)) {
        nowX = i.toDouble();
        break;
      }
    }

    final genSpots = _spots(pts, (p) => p.generation);
    final loadSpots = _spots(pts, (p) => p.load);
    final surplusSpots = _spots(pts, (p) => p.surplus);

    // Show ~8 x-axis labels regardless of data density
    final xInterval = max(1, (pts.length / 8).ceil()).toDouble();
    final yInterval = (maxY - minY) / 5;

    return LineChart(
      LineChartData(
        minX: 0,
        maxX: (pts.length - 1).toDouble(),
        minY: minY,
        maxY: maxY,
        clipData: const FlClipData.all(),

        // ── Grid ────────────────────────────────────────────────────────────
        gridData: FlGridData(
          show: true,
          drawVerticalLine: false,
          horizontalInterval: yInterval,
          getDrawingHorizontalLine: (_) => FlLine(
            color: Colors.grey.withOpacity(0.15),
            strokeWidth: 1,
          ),
        ),
        borderData: FlBorderData(
          show: true,
          border: Border(
            bottom: BorderSide(color: Colors.grey.shade300),
            left: BorderSide(color: Colors.grey.shade300),
          ),
        ),

        // ── Reference lines ─────────────────────────────────────────────────
        extraLinesData: ExtraLinesData(
          horizontalLines: [
            HorizontalLine(
              y: 0,
              color: Colors.grey.shade400,
              strokeWidth: 1,
              dashArray: [6, 4],
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
                      labelResolver: (_) => 'Now',
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
              reservedSize: 58,
              getTitlesWidget: (value, meta) {
                if (value == meta.max || value == meta.min) {
                  return const SizedBox.shrink();
                }
                return Text(
                  _fmtMW(value),
                  style:
                      TextStyle(fontSize: 10, color: Colors.grey.shade600),
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
                    style: TextStyle(
                        fontSize: 10, color: Colors.grey.shade600),
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
            getTooltipItems: (spots) {
              final t = L10n.current;
              return spots.map((spot) {
                final idx =
                    spot.x.toInt().clamp(0, pts.length - 1);
                final pt = pts[idx];
                final time = DateFormat('HH:mm').format(pt.timestamp);

                switch (spot.barIndex) {
                  case 0:
                    return LineTooltipItem(
                      '$time\n● ${t.gen}:  ${_fmtMW(spot.y)}',
                      const TextStyle(
                          color: Color(0xFF81C784),
                          fontSize: 12,
                          fontFamily: 'monospace'),
                    );
                  case 1:
                    return LineTooltipItem(
                      '● ${t.load}: ${_fmtMW(spot.y)}',
                      const TextStyle(
                          color: Color(0xFF64B5F6),
                          fontSize: 12,
                          fontFamily: 'monospace'),
                    );
                  case 2:
                    final isPos = spot.y >= 0;
                    return LineTooltipItem(
                      '● ${isPos ? t.surplus : t.deficit}: ${_fmtMW(spot.y.abs())}',
                      TextStyle(
                          color: isPos
                              ? const Color(0xFF81C784)
                              : const Color(0xFFEF9A9A),
                          fontSize: 12,
                          fontFamily: 'monospace'),
                    );
                  default:
                    return null;
                }
              }).toList();
            },
          ),
        ),

        // ── Lines ────────────────────────────────────────────────────────────
        lineBarsData: [
          // Generation — green
          LineChartBarData(
            spots: genSpots,
            color: _genColor,
            barWidth: 2.5,
            isCurved: true,
            curveSmoothness: 0.25,
            dotData: const FlDotData(show: false),
          ),
          // Load — blue
          LineChartBarData(
            spots: loadSpots,
            color: _loadColor,
            barWidth: 2.5,
            isCurved: true,
            curveSmoothness: 0.25,
            dotData: const FlDotData(show: false),
          ),
          // Surplus — orange with green fill above zero / red fill below zero
          LineChartBarData(
            spots: surplusSpots,
            color: _surplusColor,
            barWidth: 2,
            isCurved: true,
            curveSmoothness: 0.25,
            dotData: const FlDotData(show: false),
            // Green fill: surplus line → y=0 (only for positive values)
            belowBarData: BarAreaData(
              show: true,
              color: Colors.green.shade300.withOpacity(0.30),
              cutOffY: 0,
              applyCutOffY: true,
            ),
            // Red fill: y=0 → surplus line (only for negative values)
            aboveBarData: BarAreaData(
              show: true,
              color: Colors.red.shade300.withOpacity(0.28),
              cutOffY: 0,
              applyCutOffY: true,
            ),
          ),
        ],
      ),
    );
  }

  List<FlSpot> _spots(
      List<EnergyPoint> pts, double Function(EnergyPoint) fn) {
    return pts
        .asMap()
        .entries
        .map((e) => FlSpot(e.key.toDouble(), fn(e.value)))
        .toList();
  }

  String _fmtMW(double v) {
    final abs = v.abs();
    if (abs >= 1000) return '${(v / 1000).toStringAsFixed(1)} GW';
    return '${v.toStringAsFixed(0)} MW';
  }
}
