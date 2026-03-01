// API server address.
// For local mobile development, override this constant directly.
// For Docker / web builds, pass via --dart-define:
//   flutter build web --dart-define=API_URL=http://your-server:3044
const String kBaseUrl = String.fromEnvironment(
  'API_URL',
  defaultValue: 'http://37.27.47.184:3044',
);

const String kDefaultCountry = 'DE';
const int kDefaultHours = 24;
