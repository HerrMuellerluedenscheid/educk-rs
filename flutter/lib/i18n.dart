import 'dart:ui';

/// App strings, EN/DE — mirrors the I18N table in static/app/app.js so the
/// mobile app reads exactly like the web dashboard.
class L10n {
  final String code;

  final String errorTitle;
  final String retry;
  final String refresh;

  final String surplusNow;
  final String deficitNow;
  final String peakSurplus;
  final String greenCoverage;
  final String ofCurrentLoad;
  final String ofGeneration;
  final String renewableNow;

  final String rising;
  final String falling;
  final String steady;

  final String dataPoints;
  final String now;

  final String gen;
  final String load;
  final String surplus;
  final String deficit;

  final String legGen;
  final String legLoad;
  final String legSurplus;
  final String legPos;
  final String legDeficit;

  final String tabOverview;
  final String tabDetails;
  final String legCurve;
  final String renShare;

  /// Hint templates; `{t}` is replaced with the peak time.
  final String _hintGood;
  final String _hintBad;

  String hintGood(String t) => _hintGood.replaceFirst('{t}', t);
  String hintBad(String t) => _hintBad.replaceFirst('{t}', t);

  const L10n._({
    required this.code,
    required this.errorTitle,
    required this.retry,
    required this.refresh,
    required this.surplusNow,
    required this.deficitNow,
    required this.peakSurplus,
    required this.greenCoverage,
    required this.ofCurrentLoad,
    required this.ofGeneration,
    required this.renewableNow,
    required this.rising,
    required this.falling,
    required this.steady,
    required this.dataPoints,
    required this.now,
    required this.gen,
    required this.load,
    required this.surplus,
    required this.deficit,
    required this.legGen,
    required this.legLoad,
    required this.legSurplus,
    required this.legPos,
    required this.legDeficit,
    required this.tabOverview,
    required this.tabDetails,
    required this.legCurve,
    required this.renShare,
    required String hintGood,
    required String hintBad,
  })  : _hintGood = hintGood,
        _hintBad = hintBad;

  static const en = L10n._(
    code: 'en',
    errorTitle: 'Could not load data',
    retry: 'Retry',
    refresh: 'Refresh',
    surplusNow: 'Surplus now',
    deficitNow: 'Deficit now',
    peakSurplus: 'Peak surplus',
    greenCoverage: 'Green coverage',
    ofCurrentLoad: 'of current load',
    ofGeneration: 'of generation',
    renewableNow: 'renewable now',
    rising: 'Rising',
    falling: 'Falling',
    steady: 'Steady',
    dataPoints: 'data points',
    now: 'Now',
    gen: 'Gen',
    load: 'Load',
    surplus: 'Surplus',
    deficit: 'Deficit',
    legGen: 'Wind + Solar',
    legLoad: 'Total load',
    legSurplus: 'Surplus',
    legPos: 'Positive surplus',
    legDeficit: 'Deficit',
    tabOverview: 'Overview',
    tabDetails: 'Details',
    legCurve:
        'Renewable share = (wind + solar) ÷ load. Above 100 % means surplus.',
    renShare: 'Renewable',
    hintGood:
        'Best time to charge an EV or run high-energy appliances: {t} — '
        "that's when renewables exceed demand by the most.",
    hintBad:
        "Renewables don't cover full demand in this window. "
        'Highest renewable share is around {t}.',
  );

  static const de = L10n._(
    code: 'de',
    errorTitle: 'Daten konnten nicht geladen werden',
    retry: 'Erneut versuchen',
    refresh: 'Aktualisieren',
    surplusNow: 'Überschuss jetzt',
    deficitNow: 'Defizit jetzt',
    peakSurplus: 'Spitzenüberschuss',
    greenCoverage: 'Grüne Deckung',
    ofCurrentLoad: 'des aktuellen Bedarfs',
    ofGeneration: 'der Erzeugung',
    renewableNow: 'erneuerbar jetzt',
    rising: 'Steigend',
    falling: 'Fallend',
    steady: 'Stabil',
    dataPoints: 'Datenpunkte',
    now: 'Jetzt',
    gen: 'Erz.',
    load: 'Last',
    surplus: 'Überschuss',
    deficit: 'Defizit',
    legGen: 'Wind + Solar',
    legLoad: 'Gesamtlast',
    legSurplus: 'Überschuss',
    legPos: 'Positiver Überschuss',
    legDeficit: 'Defizit',
    tabOverview: 'Übersicht',
    tabDetails: 'Details',
    legCurve:
        'Erneuerbaren-Anteil = (Wind + Solar) ÷ Last. Über 100 % bedeutet Überschuss.',
    renShare: 'Erneuerbar',
    hintGood:
        'Beste Zeit zum Laden eines E-Autos oder Betreiben energieintensiver '
        'Geräte: {t} — dann übersteigen die Erneuerbaren den Bedarf am stärksten.',
    hintBad:
        'Die Erneuerbaren decken in diesem Zeitfenster nicht den gesamten '
        'Bedarf. Der höchste Erneuerbaren-Anteil liegt gegen {t}.',
  );

  /// Active language. Defaults to English; [init] switches to German when the
  /// device locale is German (same soft negotiation as the web dashboard).
  static L10n current = en;

  static void init() {
    final lang = PlatformDispatcher.instance.locale.languageCode.toLowerCase();
    current = lang.startsWith('de') ? de : en;
  }
}

/// ISO country code → full name (matches src/entsoe/areas.rs and app.js), so
/// the picker reads "Germany" instead of "DE".
const Map<String, String> kCountryNames = {
  'AL': 'Albania', 'AT': 'Austria',
  'BE': 'Belgium', 'BA': 'Bosnia and Herzegovina', 'BG': 'Bulgaria',
  'BY': 'Belarus',
  'CH': 'Switzerland', 'CY': 'Cyprus', 'CZ': 'Czech Republic', 'DE': 'Germany',
  'DK': 'Denmark', 'EE': 'Estonia', 'ES': 'Spain', 'FI': 'Finland',
  'FR': 'France',
  'GR': 'Greece', 'HR': 'Croatia', 'HU': 'Hungary',
  'IE': 'Ireland', 'IS': 'Iceland', 'IT': 'Italy', 'LT': 'Lithuania',
  'LU': 'Luxembourg',
  'LV': 'Latvia', 'MD': 'Moldova', 'ME': 'Montenegro', 'MK': 'North Macedonia',
  'MT': 'Malta', 'NL': 'Netherlands', 'NO': 'Norway', 'PL': 'Poland',
  'PT': 'Portugal',
  'RO': 'Romania', 'RS': 'Serbia', 'RU': 'Kaliningrad', 'SE': 'Sweden',
  'SI': 'Slovenia', 'SK': 'Slovakia', 'TR': 'Turkey', 'UA': 'Ukraine',
};

String countryName(String code) => kCountryNames[code] ?? code;
