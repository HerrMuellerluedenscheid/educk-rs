import 'package:flutter/material.dart';
import 'package:intl/intl.dart';
import '../i18n.dart';
import '../models/energy_data.dart';

class SummaryCards extends StatelessWidget {
  final EnergyData data;

  const SummaryCards({super.key, required this.data});

  @override
  Widget build(BuildContext context) {
    final current = data.currentPoint;
    final peak = data.peakSurplusPoint;
    final trend = data.renewableTrend;

    return Row(
      children: [
        if (current != null) ...[
          Expanded(child: _CurrentStatusCard(point: current, trend: trend)),
          const SizedBox(width: 10),
        ],
        Expanded(child: _PeakSurplusCard(point: peak)),
        const SizedBox(width: 10),
        if (current != null)
          Expanded(child: _CoverageCard(point: current)),
      ],
    );
  }
}

// ── Renewable trend symbol ─────────────────────────────────────────────────────

class RenewableTrendBadge extends StatelessWidget {
  final RenewableTrend trend;
  final double iconSize;
  final bool showLabel;

  const RenewableTrendBadge({
    super.key,
    required this.trend,
    this.iconSize = 14,
    this.showLabel = true,
  });

  @override
  Widget build(BuildContext context) {
    final t = L10n.current;
    final (icon, color, label) = switch (trend) {
      RenewableTrend.rising => (
          Icons.trending_up,
          Colors.green.shade700,
          t.rising
        ),
      RenewableTrend.falling => (
          Icons.trending_down,
          Colors.red.shade700,
          t.falling
        ),
      RenewableTrend.flat => (
          Icons.trending_flat,
          Colors.grey.shade600,
          t.steady
        ),
    };

    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Icon(icon, size: iconSize, color: color),
        if (showLabel) ...[
          const SizedBox(width: 3),
          Text(
            label,
            style: TextStyle(
              fontSize: 10,
              fontWeight: FontWeight.w600,
              color: color,
            ),
          ),
        ],
      ],
    );
  }
}

// ── Current status ─────────────────────────────────────────────────────────────

class _CurrentStatusCard extends StatelessWidget {
  final EnergyPoint point;
  final RenewableTrend trend;
  const _CurrentStatusCard({required this.point, required this.trend});

  @override
  Widget build(BuildContext context) {
    final t = L10n.current;
    final isSurplus = point.surplus >= 0;
    final label = isSurplus ? t.surplusNow : t.deficitNow;
    final color = isSurplus ? Colors.green.shade700 : Colors.red.shade700;
    final bgColor = isSurplus ? Colors.green.shade50 : Colors.red.shade50;
    final icon = isSurplus ? Icons.bolt_outlined : Icons.warning_amber_outlined;

    return _Card(
      bgColor: bgColor,
      icon: icon,
      iconColor: color,
      title: label,
      value: _fmtMW(point.surplus.abs()),
      valueColor: color,
      subtitle: '${point.surplusPercent.toStringAsFixed(0)}% ${t.ofGeneration}',
      trailing: RenewableTrendBadge(trend: trend),
    );
  }
}

// ── Peak surplus ───────────────────────────────────────────────────────────────

class _PeakSurplusCard extends StatelessWidget {
  final EnergyPoint point;
  const _PeakSurplusCard({required this.point});

  @override
  Widget build(BuildContext context) {
    final t = L10n.current;
    final time = DateFormat('HH:mm').format(point.timestamp);
    return _Card(
      bgColor: Colors.orange.shade50,
      icon: Icons.wb_sunny_outlined,
      iconColor: Colors.orange.shade700,
      title: t.peakSurplus,
      value: time,
      valueColor: Colors.orange.shade800,
      subtitle: _fmtMW(point.surplus),
    );
  }
}

// ── Renewable coverage ─────────────────────────────────────────────────────────

class _CoverageCard extends StatelessWidget {
  final EnergyPoint point;
  const _CoverageCard({required this.point});

  @override
  Widget build(BuildContext context) {
    final t = L10n.current;
    final pct = point.renewableCoverage;
    final color = _coverageColor(pct);
    return _Card(
      bgColor: color.withOpacity(0.08),
      icon: Icons.eco_outlined,
      iconColor: color,
      title: t.greenCoverage,
      value: '${pct.toStringAsFixed(0)}%',
      valueColor: color,
      subtitle: t.ofCurrentLoad,
    );
  }

  Color _coverageColor(double pct) {
    if (pct >= 80) return Colors.green.shade700;
    if (pct >= 50) return Colors.lightGreen.shade700;
    if (pct >= 30) return Colors.orange.shade700;
    return Colors.red.shade700;
  }
}

// ── Shared card shell ──────────────────────────────────────────────────────────

class _Card extends StatelessWidget {
  final Color bgColor;
  final IconData icon;
  final Color iconColor;
  final String title;
  final String value;
  final Color valueColor;
  final String subtitle;
  final Widget? trailing;

  const _Card({
    required this.bgColor,
    required this.icon,
    required this.iconColor,
    required this.title,
    required this.value,
    required this.valueColor,
    required this.subtitle,
    this.trailing,
  });

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
      decoration: BoxDecoration(
        color: bgColor,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: iconColor.withOpacity(0.25)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(icon, size: 14, color: iconColor),
              const SizedBox(width: 4),
              Expanded(
                child: Text(title,
                    style: TextStyle(
                        fontSize: 11,
                        color: Colors.grey.shade700,
                        fontWeight: FontWeight.w500),
                    overflow: TextOverflow.ellipsis),
              ),
            ],
          ),
          const SizedBox(height: 4),
          Text(value,
              style: TextStyle(
                  fontSize: 18,
                  fontWeight: FontWeight.bold,
                  color: valueColor)),
          const SizedBox(height: 2),
          Row(
            children: [
              Expanded(
                child: Text(subtitle,
                    style:
                        TextStyle(fontSize: 10, color: Colors.grey.shade500),
                    overflow: TextOverflow.ellipsis),
              ),
              if (trailing != null) ...[
                const SizedBox(width: 6),
                trailing!,
              ],
            ],
          ),
        ],
      ),
    );
  }
}

String _fmtMW(double v) {
  final abs = v.abs();
  final sign = v < 0 ? '-' : '';
  if (abs >= 1000) return '$sign${(abs / 1000).toStringAsFixed(1)} GW';
  return '$sign${abs.toStringAsFixed(0)} MW';
}
