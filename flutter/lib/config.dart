// API server address.
// Development: set API_URL in flutter/.env (see flutter/.env.example); `just dev`
// and `just run-flutter` pass it via --dart-define-from-file=.env.
// Production builds: pass --dart-define=API_URL=https://api.educk.io (takes
// precedence over the .env file).
const String kBaseUrl = String.fromEnvironment(
  'API_URL',
  defaultValue: 'https://api.educk.io',
);

const String kDefaultCountry = 'DE';
const int kDefaultHours = 24;

// Git commit the container was built from. Injected at build time via
// --dart-define=GIT_COMMIT=... (see flutter/Dockerfile). Empty in local dev.
const String kGitCommit = String.fromEnvironment('GIT_COMMIT');
