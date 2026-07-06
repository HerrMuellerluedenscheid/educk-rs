import 'dart:math';

enum RenewableTrend { rising, falling, flat }

class EnergyPoint {
  final DateTime timestamp;
  final double generation;
  final double load;
  final double surplus;

  const EnergyPoint({
    required this.timestamp,
    required this.generation,
    required this.load,
    required this.surplus,
  });

  /// Renewable generation as a share of load (capped at 100 %).
  double get renewableCoverage =>
      load > 0 ? min(generation / load, 1.0) * 100.0 : 0.0;

  /// Renewable share of load in % — the "educk curve". Uncapped: values above
  /// 100 % mean surplus.
  double get renewableShare => load > 0 ? (generation / load) * 100.0 : 0.0;

  /// Surplus as % of generation.
  double get surplusPercent =>
      generation > 0 ? (surplus / generation) * 100.0 : 0.0;
}

class EnergyData {
  final List<EnergyPoint> points;
  final String country;

  const EnergyData({required this.points, required this.country});

  factory EnergyData.fromJson(Map<String, dynamic> json, String country) {
    final data = json['data'] as Map<String, dynamic>;
    final ts = (data['timestamps'] as List).cast<String>();
    final gen =
        (data['generation'] as List).map((v) => (v as num).toDouble()).toList();
    final load =
        (data['load'] as List).map((v) => (v as num).toDouble()).toList();
    final surplus =
        (data['surplus'] as List).map((v) => (v as num).toDouble()).toList();

    final pts = List.generate(
      ts.length,
      (i) => EnergyPoint(
        timestamp: DateTime.parse(ts[i]).toLocal(),
        generation: gen[i],
        load: load[i],
        surplus: surplus[i],
      ),
    );

    return EnergyData(points: pts, country: country);
  }

  /// Data point closest to the current wall-clock time.
  EnergyPoint? get currentPoint {
    if (points.isEmpty) return null;
    final now = DateTime.now();
    return points.reduce((a, b) =>
        a.timestamp.difference(now).abs() < b.timestamp.difference(now).abs()
            ? a
            : b);
  }

  /// Data point with the highest renewable surplus.
  EnergyPoint get peakSurplusPoint =>
      points.reduce((a, b) => a.surplus > b.surplus ? a : b);

  double get maxY =>
      points.fold(0.0, (m, p) => max(m, max(p.generation, p.load)));

  double get minY => points.fold(0.0, (m, p) => min(m, p.surplus));

  /// Where the current surplus sits within today's min–max range.
  /// 0 = worst moment of the day, 1 = best.
  double get normalizedCurrentSurplus {
    final current = currentPoint;
    if (current == null || points.isEmpty) return 0.5;
    final lo = points.fold(double.infinity, (m, p) => min(m, p.surplus));
    final hi = points.fold(double.negativeInfinity, (m, p) => max(m, p.surplus));
    if (hi == lo) return 0.5;
    return ((current.surplus - lo) / (hi - lo)).clamp(0.0, 1.0);
  }

  /// Compares average renewable generation in the ~3h window before "now"
  /// against the ~3h window after "now". Falls back to first-half vs
  /// second-half of the available series when one of the windows is empty.
  RenewableTrend get renewableTrend {
    if (points.length < 4) return RenewableTrend.flat;
    final now = DateTime.now();
    const window = Duration(hours: 3);

    final past = points
        .where((p) =>
            p.timestamp.isBefore(now) &&
            p.timestamp.isAfter(now.subtract(window)))
        .map((p) => p.generation)
        .toList();
    final future = points
        .where((p) =>
            p.timestamp.isAfter(now) &&
            p.timestamp.isBefore(now.add(window)))
        .map((p) => p.generation)
        .toList();

    double pastAvg, futureAvg;
    if (past.isEmpty || future.isEmpty) {
      final mid = points.length ~/ 2;
      pastAvg = points.take(mid).map((p) => p.generation).reduce((a, b) => a + b) /
          mid;
      futureAvg = points
              .skip(mid)
              .map((p) => p.generation)
              .reduce((a, b) => a + b) /
          (points.length - mid);
    } else {
      pastAvg = past.reduce((a, b) => a + b) / past.length;
      futureAvg = future.reduce((a, b) => a + b) / future.length;
    }

    if (pastAvg <= 0) {
      return futureAvg > 0 ? RenewableTrend.rising : RenewableTrend.flat;
    }
    final change = (futureAvg - pastAvg) / pastAvg;
    if (change > 0.05) return RenewableTrend.rising;
    if (change < -0.05) return RenewableTrend.falling;
    return RenewableTrend.flat;
  }

  /// Best upcoming (future) surplus point; falls back to overall peak.
  EnergyPoint get upcomingPeakPoint {
    final now = DateTime.now();
    final future = points.where((p) => p.timestamp.isAfter(now)).toList();
    if (future.isEmpty) return peakSurplusPoint;
    return future.reduce((a, b) => a.surplus > b.surplus ? a : b);
  }
}
