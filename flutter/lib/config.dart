// API server address.
// Development: set API_URL in flutter/.env (see flutter/.env.example); `just dev`
// and `just run-flutter` pass it via --dart-define-from-file=.env.
// Production builds: pass --dart-define=API_URL=http://your-server:3044 (takes
// precedence over the .env file).
const String kBaseUrl = String.fromEnvironment(
  'API_URL',
  defaultValue: 'http://37.27.47.184:3044',
);

const String kDefaultCountry = 'DE';
const int kDefaultHours = 24;
