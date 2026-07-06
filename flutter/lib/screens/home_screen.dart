import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:intl/intl.dart';
import '../config.dart';
import '../i18n.dart';
import '../models/energy_data.dart';
import '../services/api_service.dart';
import '../widgets/energy_chart.dart';
import '../widgets/overview_chart.dart';
import '../widgets/summary_cards.dart';
import '../widgets/surplus_gauge.dart';

/// "overview" (educk curve) or "details" (gen/load/surplus) — same two chart
/// modes as the web dashboard.
enum ChartMode { overview, details }

class HomeScreen extends StatefulWidget {
  const HomeScreen({super.key});

  @override
  State<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends State<HomeScreen> {
  late final ApiService _api;
  String _country = kDefaultCountry;
  List<String> _countries = [kDefaultCountry];
  ChartMode _mode = ChartMode.overview;
  late Future<EnergyData> _dataFuture;

  @override
  void initState() {
    super.initState();
    _api = const ApiService(baseUrl: kBaseUrl);
    _dataFuture = _fetch();
    _loadCountries();
  }

  Future<EnergyData> _fetch() =>
      _api.fetchEnergyData(_country, hours: kDefaultHours);

  void _refresh() => setState(() => _dataFuture = _fetch());

  Future<void> _loadCountries() async {
    try {
      final list = await _api.fetchCountries();
      // Sort by display name, like the web dropdown.
      list.sort((a, b) => countryName(a).compareTo(countryName(b)));
      if (mounted) {
        setState(() {
          _countries = list;
          if (!_countries.contains(_country)) {
            _country = _countries.first;
            _dataFuture = _fetch();
          }
        });
      }
    } catch (_) {
      // Country list is optional — fall back to single default entry
    }
  }

  void _changeCountry(String c) => setState(() {
        _country = c;
        _dataFuture = _fetch();
      });

  @override
  Widget build(BuildContext context) {
    // Cap the content column to a phone-like width so tablets get the same
    // proportions as phones (matches the web's max-w-3xl centering).
    return Scaffold(
      backgroundColor: Theme.of(context).colorScheme.surface,
      body: SafeArea(
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 520),
            child: Column(
              children: [
                // ── Header: brand left, country + refresh right ──────────────
                // Kept outside the FutureBuilder so the country selector stays
                // usable when a region has no data and the fetch errors out.
                Padding(
                  padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
                  child: Row(
                    children: [
                      Text(
                        'educk',
                        style: TextStyle(
                          fontSize: 18,
                          fontWeight: FontWeight.w700,
                          color: Colors.green.shade800,
                        ),
                      ),
                      const Spacer(),
                      _CountryDropdown(
                        selected: _country,
                        countries: _countries,
                        onChanged: _changeCountry,
                      ),
                      IconButton(
                        icon: const Icon(Icons.refresh, size: 20),
                        tooltip: L10n.current.refresh,
                        onPressed: _refresh,
                      ),
                    ],
                  ),
                ),
                Expanded(
                  child: RefreshIndicator(
                    onRefresh: () async => _refresh(),
                    child: FutureBuilder<EnergyData>(
                      future: _dataFuture,
                      builder: (context, snap) {
                        if (snap.connectionState == ConnectionState.waiting) {
                          return const Center(
                              child: CircularProgressIndicator());
                        }
                        if (snap.hasError) {
                          return _ErrorView(
                            error: snap.error.toString(),
                            onRetry: _refresh,
                          );
                        }
                        return _Dashboard(
                          data: snap.data!,
                          mode: _mode,
                          onModeChanged: (m) => setState(() => _mode = m),
                        );
                      },
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

// ── Dashboard body (mirrors the web layout top to bottom) ─────────────────────

class _Dashboard extends StatelessWidget {
  final EnergyData data;
  final ChartMode mode;
  final ValueChanged<ChartMode> onModeChanged;

  const _Dashboard({
    required this.data,
    required this.mode,
    required this.onModeChanged,
  });

  @override
  Widget build(BuildContext context) {
    final t = L10n.current;
    final norm = data.normalizedCurrentSurplus;
    final coverage = data.currentPoint?.renewableCoverage ?? 0;

    return ListView(
      padding: const EdgeInsets.fromLTRB(16, 12, 16, 32),
      children: [
        // ── Gauge ────────────────────────────────────────────────────────
        Center(
          child: FractionallySizedBox(
            widthFactor: 0.55,
            child: SurplusGauge(value: norm, coveragePct: coverage),
          ),
        ),

        const SizedBox(height: 12),

        // ── Summary cards ────────────────────────────────────────────────
        SummaryCards(data: data),

        const SizedBox(height: 16),

        // ── Time range + view toggle ─────────────────────────────────────
        Row(
          children: [
            Expanded(child: _RangeText(data: data)),
            const SizedBox(width: 8),
            _ModeTabs(mode: mode, onChanged: onModeChanged),
          ],
        ),

        const SizedBox(height: 4),

        // ── Chart ────────────────────────────────────────────────────────
        SizedBox(
          height: 280,
          child: mode == ChartMode.overview
              ? OverviewChart(data: data)
              : EnergyChart(data: data),
        ),

        const SizedBox(height: 12),

        // ── Legend ───────────────────────────────────────────────────────
        if (mode == ChartMode.overview)
          Text(
            t.legCurve,
            style: TextStyle(fontSize: 11, color: Colors.grey.shade500),
          )
        else
          const _DetailsLegend(),

        const SizedBox(height: 20),

        // ── Interpretation hint ──────────────────────────────────────────
        _InterpretationHint(data: data),

        const SizedBox(height: 24),
        const _BuildFooter(),
      ],
    );
  }
}

// ── Time range subtitle ────────────────────────────────────────────────────────

class _RangeText extends StatelessWidget {
  final EnergyData data;
  const _RangeText({required this.data});

  @override
  Widget build(BuildContext context) {
    final t = L10n.current;
    if (data.points.isEmpty) return const SizedBox.shrink();
    final first = data.points.first.timestamp;
    final last = data.points.last.timestamp;
    final day = DateFormat('EEE d MMM').format(first);
    final hm = DateFormat('HH:mm');
    return Text(
      '$day, ${hm.format(first)} → ${hm.format(last)} '
      '(${data.points.length} ${t.dataPoints})',
      style: TextStyle(fontSize: 11, color: Colors.grey.shade500),
    );
  }
}

// ── Overview / Details toggle (styled like the web tab pill) ──────────────────

class _ModeTabs extends StatelessWidget {
  final ChartMode mode;
  final ValueChanged<ChartMode> onChanged;

  const _ModeTabs({required this.mode, required this.onChanged});

  @override
  Widget build(BuildContext context) {
    final t = L10n.current;
    final isDark = Theme.of(context).brightness == Brightness.dark;
    return Container(
      padding: const EdgeInsets.all(2),
      decoration: BoxDecoration(
        color: isDark ? Colors.white.withOpacity(0.06) : Colors.white,
        borderRadius: BorderRadius.circular(10),
        border: Border.all(
          color: isDark ? Colors.white.withOpacity(0.12) : Colors.grey.shade300,
        ),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          _tab(context, t.tabOverview, ChartMode.overview),
          _tab(context, t.tabDetails, ChartMode.details),
        ],
      ),
    );
  }

  Widget _tab(BuildContext context, String label, ChartMode m) {
    final active = m == mode;
    return InkWell(
      onTap: () => onChanged(m),
      borderRadius: BorderRadius.circular(8),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 5),
        decoration: BoxDecoration(
          color: active ? const Color(0xFF059669) : Colors.transparent,
          borderRadius: BorderRadius.circular(8),
        ),
        child: Text(
          label,
          style: TextStyle(
            fontSize: 11,
            fontWeight: FontWeight.w600,
            color: active
                ? Colors.white
                : Theme.of(context).colorScheme.onSurface.withOpacity(0.65),
          ),
        ),
      ),
    );
  }
}

// ── Details chart legend ───────────────────────────────────────────────────────

class _DetailsLegend extends StatelessWidget {
  const _DetailsLegend();

  @override
  Widget build(BuildContext context) {
    final t = L10n.current;
    return Wrap(
      spacing: 20,
      runSpacing: 8,
      children: [
        _LegendItem(color: const Color(0xFF2E7D32), label: t.legGen),
        _LegendItem(color: const Color(0xFF1565C0), label: t.legLoad),
        _LegendItem(color: const Color(0xFFF57C00), label: t.legSurplus),
        _LegendAreaItem(
            fill: const Color(0x4D81C784),
            border: const Color(0xFF2E7D32),
            label: t.legPos),
        _LegendAreaItem(
            fill: const Color(0x47EF9A9A),
            border: const Color(0xFFC62828),
            label: t.legDeficit),
      ],
    );
  }
}

class _LegendItem extends StatelessWidget {
  final Color color;
  final String label;
  const _LegendItem({required this.color, required this.label});

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Container(
          width: 22,
          height: 3,
          decoration: BoxDecoration(
            color: color,
            borderRadius: BorderRadius.circular(2),
          ),
        ),
        const SizedBox(width: 6),
        Text(label,
            style: TextStyle(fontSize: 11, color: Colors.grey.shade700)),
      ],
    );
  }
}

class _LegendAreaItem extends StatelessWidget {
  final Color fill;
  final Color border;
  final String label;
  const _LegendAreaItem(
      {required this.fill, required this.border, required this.label});

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Container(
          width: 14,
          height: 14,
          decoration: BoxDecoration(
            color: fill,
            border: Border.all(color: border, width: 1.5),
            borderRadius: BorderRadius.circular(3),
          ),
        ),
        const SizedBox(width: 6),
        Text(label,
            style: TextStyle(fontSize: 11, color: Colors.grey.shade700)),
      ],
    );
  }
}

// ── Interpretation hint ────────────────────────────────────────────────────────

class _InterpretationHint extends StatelessWidget {
  final EnergyData data;
  const _InterpretationHint({required this.data});

  @override
  Widget build(BuildContext context) {
    final t = L10n.current;
    final peak = data.peakSurplusPoint;
    final isGood = peak.surplus > 0;
    final peakTime = DateFormat('HH:mm').format(peak.timestamp);

    return Container(
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: isGood ? Colors.green.shade50 : Colors.orange.shade50,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(
          color: isGood ? Colors.green.shade200 : Colors.orange.shade200,
        ),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(
            isGood ? Icons.lightbulb_outline : Icons.info_outline,
            color: isGood ? Colors.green.shade700 : Colors.orange.shade700,
            size: 20,
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Text(
              isGood ? t.hintGood(peakTime) : t.hintBad(peakTime),
              style: TextStyle(
                fontSize: 13,
                color: Colors.grey.shade800,
                height: 1.4,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

// ── Country dropdown (full names, like the web) ────────────────────────────────

class _CountryDropdown extends StatelessWidget {
  final String selected;
  final List<String> countries;
  final ValueChanged<String> onChanged;

  const _CountryDropdown({
    required this.selected,
    required this.countries,
    required this.onChanged,
  });

  @override
  Widget build(BuildContext context) {
    return DropdownButtonHideUnderline(
      child: DropdownButton<String>(
        value: countries.contains(selected) ? selected : countries.first,
        items: countries
            .map((c) => DropdownMenuItem(value: c, child: Text(countryName(c))))
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
    final t = L10n.current;
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(32),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.cloud_off, size: 56, color: Colors.grey.shade400),
            const SizedBox(height: 16),
            Text(t.errorTitle, style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            Text(error,
                textAlign: TextAlign.center,
                style: TextStyle(fontSize: 13, color: Colors.grey.shade600)),
            const SizedBox(height: 24),
            FilledButton.icon(
              onPressed: onRetry,
              icon: const Icon(Icons.refresh),
              label: Text(t.retry),
            ),
          ],
        ),
      ),
    );
  }
}
