import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:intl/intl.dart';
import '../config.dart';
import '../models/energy_data.dart';
import '../services/api_service.dart';
import '../widgets/surplus_gauge.dart';
import '../widgets/mini_forecast.dart';
import 'dashboard_screen.dart';

class HomeScreen extends StatefulWidget {
  const HomeScreen({super.key});

  @override
  State<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends State<HomeScreen> {
  late final ApiService _api;
  String _country = kDefaultCountry;
  List<String> _countries = [kDefaultCountry];
  late Future<EnergyData> _dataFuture;

  @override
  void initState() {
    super.initState();
    _api = ApiService(baseUrl: kBaseUrl);
    _dataFuture = _fetch();
    _loadCountries();
  }

  Future<EnergyData> _fetch() => _api.fetchEnergyData(_country, hours: 24);

  void _refresh() => setState(() => _dataFuture = _fetch());

  Future<void> _loadCountries() async {
    try {
      final list = await _api.fetchCountries();
      if (mounted) {
        setState(() {
          _countries = list;
          if (!_countries.contains(_country)) {
            _country = _countries.first;
            _dataFuture = _fetch();
          }
        });
      }
    } catch (_) {}
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: Theme.of(context).colorScheme.surface,
      body: SafeArea(
        child: RefreshIndicator(
          onRefresh: () async => _refresh(),
          child: FutureBuilder<EnergyData>(
            future: _dataFuture,
            builder: (context, snap) {
              if (snap.connectionState == ConnectionState.waiting) {
                return const Center(child: CircularProgressIndicator());
              }
              if (snap.hasError) {
                return _ErrorView(
                  error: snap.error.toString(),
                  onRetry: _refresh,
                );
              }
              return _HomeBody(
                data: snap.data!,
                country: _country,
                countries: _countries,
                onCountryChanged: (c) => setState(() {
                  _country = c;
                  _dataFuture = _fetch();
                }),
                onViewDetails: () => Navigator.push(
                  context,
                  MaterialPageRoute(
                    builder: (_) => DashboardScreen(initialCountry: _country),
                  ),
                ),
              );
            },
          ),
        ),
      ),
    );
  }
}

// ── Main body ──────────────────────────────────────────────────────────────────

class _HomeBody extends StatelessWidget {
  final EnergyData data;
  final String country;
  final List<String> countries;
  final ValueChanged<String> onCountryChanged;
  final VoidCallback onViewDetails;

  const _HomeBody({
    required this.data,
    required this.country,
    required this.countries,
    required this.onCountryChanged,
    required this.onViewDetails,
  });

  @override
  Widget build(BuildContext context) {
    final norm = data.normalizedCurrentSurplus;
    final coverage = data.currentPoint?.renewableCoverage ?? 0;
    final status = _statusInfo(norm);
    final peak = data.upcomingPeakPoint;
    final peakTime = DateFormat('HH:mm').format(peak.timestamp);

    // Cap the content column to a phone-like width so wide browsers get the
    // same proportions as mobile (the gauge scales with column width, so an
    // unconstrained ListView would blow it up while body text stayed tiny).
    return Center(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 480),
        child: ListView(
          padding: const EdgeInsets.fromLTRB(24, 16, 24, 32),
          children: [
            // ── Top bar ──────────────────────────────────────────────────────────
            Row(
              mainAxisAlignment: MainAxisAlignment.spaceBetween,
              children: [
                Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      _greeting(),
                      style: Theme.of(context).textTheme.titleMedium?.copyWith(
                            fontWeight: FontWeight.w600,
                          ),
                    ),
                    Text(
                      DateFormat('EEEE, d MMMM').format(DateTime.now()),
                      style: Theme.of(context).textTheme.bodySmall?.copyWith(
                            color: Colors.grey.withOpacity(0.6),
                          ),
                    ),
                  ],
                ),
                _CountryChip(
                  country: country,
                  countries: countries,
                  onChanged: onCountryChanged,
                ),
              ],
            ),

            const SizedBox(height: 28),

            // ── Gauge ─────────────────────────────────────────────────────────────
            Center(
              child: FractionallySizedBox(
                widthFactor: 0.5,
                child: SurplusGauge(value: norm, coveragePct: coverage),
              ),
            ),

            const SizedBox(height: 16),

            // ── Status badge ──────────────────────────────────────────────────────
            Center(
              child: Container(
                padding:
                    const EdgeInsets.symmetric(horizontal: 16, vertical: 6),
                decoration: BoxDecoration(
                  color: status.color.withOpacity(0.12),
                  borderRadius: BorderRadius.circular(20),
                  border: Border.all(color: status.color.withOpacity(0.35)),
                ),
                child: Text(
                  status.label.toUpperCase(),
                  style: TextStyle(
                    color: status.color,
                    fontWeight: FontWeight.w700,
                    fontSize: 12,
                    letterSpacing: 1.2,
                  ),
                ),
              ),
            ),

            const SizedBox(height: 16),

            // ── Message ───────────────────────────────────────────────────────────
            Text(
              _message(coverage.round(), country, norm),
              textAlign: TextAlign.center,
              style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                    height: 1.5,
                    color: Theme.of(context)
                        .colorScheme
                        .onSurface
                        .withOpacity(0.75),
                  ),
            ),

            const SizedBox(height: 8),

            // ── Best upcoming time ─────────────────────────────────────────────────
            if (peak.timestamp.isAfter(DateTime.now()))
              Center(
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Icon(Icons.bolt, size: 14, color: Colors.amber.shade600),
                    const SizedBox(width: 4),
                    Text(
                      'Best upcoming window: $peakTime',
                      style: TextStyle(
                        fontSize: 13,
                        fontWeight: FontWeight.w500,
                        color: Colors.amber.shade700,
                      ),
                    ),
                  ],
                ),
              ),

            const SizedBox(height: 28),

            // ── 24 h forecast card ────────────────────────────────────────────────
            _ForecastCard(data: data),

            const SizedBox(height: 28),

            // ── Details link ──────────────────────────────────────────────────────
            Center(
              child: TextButton.icon(
                onPressed: onViewDetails,
                icon: const Icon(Icons.bar_chart_rounded, size: 16),
                label: const Text('View full analysis'),
                style: TextButton.styleFrom(
                  foregroundColor:
                      Theme.of(context).colorScheme.onSurface.withOpacity(0.55),
                ),
              ),
            ),

            // ── Build footer ──────────────────────────────────────────────────────
            const SizedBox(height: 16),
            const _BuildFooter(),
          ],
        ),
      ),
    );
  }

  // ── Helpers ─────────────────────────────────────────────────────────────────

  static String _greeting() {
    final h = DateTime.now().hour;
    if (h < 12) return 'Good morning';
    if (h < 18) return 'Good afternoon';
    return 'Good evening';
  }

  static ({String label, Color color}) _statusInfo(double norm) {
    if (norm > 0.66) {
      return (label: 'Excellent', color: const Color(0xFF43A047));
    }
    if (norm > 0.33) {
      return (label: 'Good', color: const Color(0xFFFB8C00));
    }
    return (label: 'Low', color: const Color(0xFFE53935));
  }

  static String _message(int pct, String country, double norm) {
    if (norm > 0.66) {
      return 'Renewables are covering $pct% of $country\'s electricity — a great time to charge your EV or run appliances.';
    }
    if (norm > 0.33) {
      return 'Renewables cover $pct% of $country\'s electricity demand right now. Decent conditions.';
    }
    return 'Only $pct% of $country\'s electricity comes from renewables right now. Try shifting usage to a greener window.';
  }
}

// ── Forecast card ──────────────────────────────────────────────────────────────

class _ForecastCard extends StatelessWidget {
  final EnergyData data;

  const _ForecastCard({required this.data});

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      padding: const EdgeInsets.fromLTRB(16, 16, 16, 8),
      decoration: BoxDecoration(
        color: scheme.surfaceContainerHighest.withOpacity(0.5),
        borderRadius: BorderRadius.circular(20),
        border: Border.all(
          color: scheme.outlineVariant.withOpacity(0.4),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(Icons.timeline,
                  size: 14, color: Colors.grey.withOpacity(0.6)),
              const SizedBox(width: 6),
              Text(
                'Next 24 hours',
                style: TextStyle(
                  fontSize: 12,
                  fontWeight: FontWeight.w600,
                  color: Colors.grey.withOpacity(0.7),
                  letterSpacing: 0.3,
                ),
              ),
            ],
          ),
          const SizedBox(height: 12),
          SizedBox(
            height: 200,
            child: MiniForecast(data: data),
          ),
        ],
      ),
    );
  }
}

// ── Country chip ───────────────────────────────────────────────────────────────

class _CountryChip extends StatelessWidget {
  final String country;
  final List<String> countries;
  final ValueChanged<String> onChanged;

  const _CountryChip({
    required this.country,
    required this.countries,
    required this.onChanged,
  });

  @override
  Widget build(BuildContext context) {
    return DropdownButtonHideUnderline(
      child: DropdownButton<String>(
        value: countries.contains(country) ? country : countries.first,
        items: countries
            .map((c) => DropdownMenuItem(value: c, child: Text(c)))
            .toList(),
        onChanged: (v) {
          if (v != null) onChanged(v);
        },
        style: Theme.of(context).textTheme.bodyMedium?.copyWith(
              fontWeight: FontWeight.w600,
            ),
        borderRadius: BorderRadius.circular(12),
        icon: const Icon(Icons.keyboard_arrow_down, size: 18),
      ),
    );
  }
}

// ── Build footer ───────────────────────────────────────────────────────────────

class _BuildFooter extends StatelessWidget {
  const _BuildFooter();

  @override
  Widget build(BuildContext context) {
    final commit = kGitCommit.isEmpty ? 'dev' : kGitCommit;
    final short = commit.length > 7 ? commit.substring(0, 7) : commit;
    final color = Theme.of(context).colorScheme.onSurface.withOpacity(0.3);

    return Center(
      child: Tooltip(
        message: 'Build $commit',
        child: InkWell(
          onTap: () => Clipboard.setData(ClipboardData(text: commit)),
          borderRadius: BorderRadius.circular(4),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
            child: Text(
              'build $short',
              style: TextStyle(
                fontSize: 10,
                color: color,
                fontFamily: 'monospace',
                letterSpacing: 0.3,
              ),
            ),
          ),
        ),
      ),
    );
  }
}

// ── Error view ─────────────────────────────────────────────────────────────────

class _ErrorView extends StatelessWidget {
  final String error;
  final VoidCallback onRetry;

  const _ErrorView({required this.error, required this.onRetry});

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(32),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.cloud_off, size: 56, color: Colors.grey.shade400),
            const SizedBox(height: 16),
            Text('Could not load data',
                style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            Text(error,
                textAlign: TextAlign.center,
                style: TextStyle(fontSize: 13, color: Colors.grey.shade600)),
            const SizedBox(height: 24),
            FilledButton.icon(
              onPressed: onRetry,
              icon: const Icon(Icons.refresh),
              label: const Text('Retry'),
            ),
          ],
        ),
      ),
    );
  }
}
