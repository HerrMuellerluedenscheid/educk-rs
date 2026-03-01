import 'package:flutter/material.dart';
import 'package:intl/intl.dart';
import '../config.dart';
import '../models/energy_data.dart';
import '../services/api_service.dart';
import '../widgets/energy_chart.dart';
import '../widgets/summary_cards.dart';

class DashboardScreen extends StatefulWidget {
  final String? initialCountry;

  const DashboardScreen({super.key, this.initialCountry});

  @override
  State<DashboardScreen> createState() => _DashboardScreenState();
}

class _DashboardScreenState extends State<DashboardScreen> {
  late final ApiService _api;

  late String _country;
  int _hours = kDefaultHours;
  List<String> _countries = [kDefaultCountry];
  bool _countriesLoaded = false;

  late Future<EnergyData> _dataFuture;

  @override
  void initState() {
    super.initState();
    _country = widget.initialCountry ?? kDefaultCountry;
    _api = ApiService(baseUrl: kBaseUrl);
    _dataFuture = _fetch();
    _loadCountries();
  }

  Future<EnergyData> _fetch() =>
      _api.fetchEnergyData(_country, hours: _hours);

  void _refresh() => setState(() => _dataFuture = _fetch());

  Future<void> _loadCountries() async {
    try {
      final list = await _api.fetchCountries();
      if (mounted) {
        setState(() {
          _countries = list;
          _countriesLoaded = true;
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

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: Theme.of(context).colorScheme.surface,
      appBar: _buildAppBar(),
      body: RefreshIndicator(
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
            final data = snap.data!;
            return _DataView(data: data, hours: _hours);
          },
        ),
      ),
    );
  }

  AppBar _buildAppBar() {
    return AppBar(
      title: const Text(
        'Energy Dashboard',
        style: TextStyle(fontWeight: FontWeight.w600),
      ),
      backgroundColor: Theme.of(context).colorScheme.surface,
      elevation: 0,
      scrolledUnderElevation: 1,
      actions: [
        // Hours selector
        _HoursPicker(
          value: _hours,
          onChanged: (h) => setState(() {
            _hours = h;
            _dataFuture = _fetch();
          }),
        ),
        // Country dropdown
        Padding(
          padding: const EdgeInsets.only(right: 8),
          child: _CountryDropdown(
            selected: _country,
            countries: _countriesLoaded ? _countries : [_country],
            onChanged: (c) => setState(() {
              _country = c;
              _dataFuture = _fetch();
            }),
          ),
        ),
        // Refresh button
        IconButton(
          icon: const Icon(Icons.refresh),
          tooltip: 'Refresh',
          onPressed: _refresh,
        ),
      ],
    );
  }
}

// ── Data view (summary + chart + legend) ──────────────────────────────────────

class _DataView extends StatelessWidget {
  final EnergyData data;
  final int hours;

  const _DataView({required this.data, required this.hours});

  @override
  Widget build(BuildContext context) {
    final first = data.points.first.timestamp;
    final last = data.points.last.timestamp;
    final fmt = DateFormat('EEE d MMM, HH:mm');

    return ListView(
      padding: const EdgeInsets.only(bottom: 24),
      children: [
        // Summary cards
        SummaryCards(data: data),

        // Subtitle: time range
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
          child: Text(
            '${fmt.format(first)} → ${DateFormat('HH:mm').format(last)}  '
            '(${data.points.length} data points)',
            style: TextStyle(fontSize: 11, color: Colors.grey.shade500),
          ),
        ),

        // Chart
        Padding(
          padding: const EdgeInsets.fromLTRB(8, 0, 16, 0),
          child: SizedBox(
            height: 280,
            child: EnergyChart(data: data),
          ),
        ),

        const SizedBox(height: 16),

        // Legend
        const _Legend(),

        const SizedBox(height: 24),

        // Interpretation hint
        _InterpretationHint(data: data),
      ],
    );
  }
}

// ── Chart legend ───────────────────────────────────────────────────────────────

class _Legend extends StatelessWidget {
  const _Legend();

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16),
      child: Wrap(
        spacing: 20,
        runSpacing: 8,
        children: const [
          _LegendItem(color: Color(0xFF2E7D32), label: 'Wind + Solar (gen.)'),
          _LegendItem(color: Color(0xFF1565C0), label: 'Total load'),
          _LegendItem(color: Color(0xFFF57C00), label: 'Surplus (gen − load)'),
          _LegendAreaItem(
              fill: Color(0x4D81C784),
              border: Color(0xFF2E7D32),
              label: 'Positive surplus'),
          _LegendAreaItem(
              fill: Color(0x47EF9A9A),
              border: Color(0xFFC62828),
              label: 'Deficit'),
        ],
      ),
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
          width: 22, height: 3,
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
          width: 14, height: 14,
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
    final peak = data.peakSurplusPoint;
    final isGood = peak.surplus > 0;
    final peakTime = DateFormat('HH:mm').format(peak.timestamp);

    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16),
      child: Container(
        padding: const EdgeInsets.all(14),
        decoration: BoxDecoration(
          color: isGood
              ? Colors.green.shade50
              : Colors.orange.shade50,
          borderRadius: BorderRadius.circular(12),
          border: Border.all(
            color: isGood
                ? Colors.green.shade200
                : Colors.orange.shade200,
          ),
        ),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Icon(
              isGood
                  ? Icons.lightbulb_outline
                  : Icons.info_outline,
              color: isGood
                  ? Colors.green.shade700
                  : Colors.orange.shade700,
              size: 20,
            ),
            const SizedBox(width: 10),
            Expanded(
              child: Text(
                isGood
                    ? 'Best time to charge EV / run high-energy appliances: '
                        '$peakTime — that\'s when renewables exceed demand '
                        'by the most.'
                    : 'Renewables don\'t cover full demand in this window. '
                        'Highest renewable share is around $peakTime.',
                style: TextStyle(
                  fontSize: 13,
                  color: Colors.grey.shade800,
                  height: 1.4,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

// ── Country dropdown ───────────────────────────────────────────────────────────

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
            .map((c) => DropdownMenuItem(value: c, child: Text(c)))
            .toList(),
        onChanged: (v) {
          if (v != null) onChanged(v);
        },
        style: const TextStyle(fontWeight: FontWeight.w600, fontSize: 14),
        icon: const Icon(Icons.arrow_drop_down, size: 20),
      ),
    );
  }
}

// ── Hours picker ───────────────────────────────────────────────────────────────

class _HoursPicker extends StatelessWidget {
  final int value;
  final ValueChanged<int> onChanged;

  const _HoursPicker({required this.value, required this.onChanged});

  static const _options = [6, 12, 24, 48];

  @override
  Widget build(BuildContext context) {
    return PopupMenuButton<int>(
      initialValue: value,
      onSelected: onChanged,
      tooltip: 'Time window',
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 8),
        child: Row(
          children: [
            const Icon(Icons.schedule, size: 18),
            const SizedBox(width: 4),
            Text('${value}h',
                style: const TextStyle(
                    fontWeight: FontWeight.w600, fontSize: 14)),
          ],
        ),
      ),
      itemBuilder: (_) => _options
          .map((h) => PopupMenuItem(value: h, child: Text('${h}h')))
          .toList(),
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
            Text(
              error,
              textAlign: TextAlign.center,
              style: TextStyle(fontSize: 13, color: Colors.grey.shade600),
            ),
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
