import 'dart:math';
import 'package:flutter/material.dart';

/// Speedometer-style arc gauge.
///
/// [value]      – normalised position on the arc (0 = worst, 1 = best today).
/// [coveragePct]– actual renewable coverage % shown as the centre label.
class SurplusGauge extends StatelessWidget {
  final double value;
  final double coveragePct;

  const SurplusGauge({
    super.key,
    required this.value,
    required this.coveragePct,
  });

  @override
  Widget build(BuildContext context) {
    final isDark = Theme.of(context).brightness == Brightness.dark;
    final trackColor = isDark
        ? Colors.white.withOpacity(0.08)
        : Colors.black.withOpacity(0.07);

    return LayoutBuilder(builder: (_, bc) {
      final w = bc.maxWidth;
      // Geometry (all relative to w so it scales)
      final r = w * 0.36;
      final tw = w * 0.062;
      final cx = w / 2;
      final cy = w * 0.04 + tw / 2 + r; // top-padding + half-stroke + radius
      final h = cy + r * 0.52 + tw / 2 + w * 0.04; // bottom padding

      return SizedBox(
        width: w,
        height: h,
        child: Stack(
          children: [
            Positioned.fill(
              child: CustomPaint(
                painter: _GaugePainter(
                  value: value.clamp(0.0, 1.0),
                  trackColor: trackColor,
                  radius: r,
                  strokeWidth: tw,
                  center: Offset(cx, cy),
                ),
              ),
            ),
            // Centre label – positioned at arc circle centre
            Positioned(
              left: 0,
              right: 0,
              top: cy - h * 0.22, // visually centre inside the arc
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text(
                    '${coveragePct.round()}%',
                    textAlign: TextAlign.center,
                    style: TextStyle(
                      fontSize: w * 0.16,
                      fontWeight: FontWeight.w700,
                      letterSpacing: -1.5,
                      height: 1,
                    ),
                  ),
                  const SizedBox(height: 5),
                  Text(
                    'renewable now',
                    textAlign: TextAlign.center,
                    style: TextStyle(
                      fontSize: w * 0.038,
                      fontWeight: FontWeight.w500,
                      color: Colors.grey.withOpacity(0.6),
                      letterSpacing: 0.4,
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
      );
    });
  }
}

// ── Painter ────────────────────────────────────────────────────────────────────

class _GaugePainter extends CustomPainter {
  final double value; // 0..1
  final Color trackColor;
  final double radius;
  final double strokeWidth;
  final Offset center;

  // Arc: 150 ° → 390 ° clockwise (240 ° sweep, like a speedometer)
  static const double _startDeg = 150.0;
  static const double _sweepDeg = 240.0;
  static final double _startRad = _startDeg * pi / 180;
  static final double _sweepRad = _sweepDeg * pi / 180;

  const _GaugePainter({
    required this.value,
    required this.trackColor,
    required this.radius,
    required this.strokeWidth,
    required this.center,
  });

  @override
  void paint(Canvas canvas, Size size) {
    final arcRect = Rect.fromCircle(center: center, radius: radius);

    // 1 ── Background track
    canvas.drawArc(
      arcRect,
      _startRad,
      _sweepRad,
      false,
      Paint()
        ..color = trackColor
        ..style = PaintingStyle.stroke
        ..strokeWidth = strokeWidth
        ..strokeCap = StrokeCap.round,
    );

    if (value <= 0) return;

    // 2 ── Gradient progress arc
    final progressSweep = _sweepRad * value;

    final shader = SweepGradient(
      center: Alignment.center,
      startAngle: _startRad,
      endAngle: _startRad + _sweepRad, // gradient spans the full arc extent
      colors: const [
        Color(0xFFE53935), // red   – low
        Color(0xFFFB8C00), // amber – mid
        Color(0xFF43A047), // green – high
      ],
      stops: const [0.0, 0.5, 1.0],
    ).createShader(arcRect);

    canvas.drawArc(
      arcRect,
      _startRad,
      progressSweep,
      false,
      Paint()
        ..shader = shader
        ..style = PaintingStyle.stroke
        ..strokeWidth = strokeWidth
        ..strokeCap = StrokeCap.round,
    );

    // 3 ── Glowing dot at the tip of the progress arc
    final tipAngle = _startRad + progressSweep;
    final tip = Offset(
      center.dx + radius * cos(tipAngle),
      center.dy + radius * sin(tipAngle),
    );
    // Outer glow
    canvas.drawCircle(
      tip,
      strokeWidth * 0.75,
      Paint()
        ..color = Colors.white.withOpacity(0.25)
        ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 6),
    );
    // Solid dot
    canvas.drawCircle(
      tip,
      strokeWidth * 0.42,
      Paint()..color = Colors.white.withOpacity(0.95),
    );
  }

  @override
  bool shouldRepaint(_GaugePainter old) =>
      old.value != value ||
      old.trackColor != trackColor ||
      old.radius != radius ||
      old.center != center;
}
