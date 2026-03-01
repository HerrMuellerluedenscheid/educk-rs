# Energy Dashboard — Setup

## 1. Scaffold the Flutter project

```sh
cd energy_dashboard
flutter create . --project-name energy_dashboard --org com.example
# This generates android/, ios/, web/, etc.
# The lib/ files you already have will be kept.
flutter pub get
```

## 2. Configure the API URL

Edit `lib/config.dart`:

| Target                    | `kBaseUrl`                          |
|---------------------------|-------------------------------------|
| Android emulator          | `http://10.0.2.2:3044`  ✓ default  |
| iOS simulator             | `http://localhost:3044`             |
| Physical device (same LAN)| `http://<your-machine-ip>:3044`     |

## 3. Allow plain HTTP (the API server uses HTTP, not HTTPS)

### Android
In `android/app/src/main/AndroidManifest.xml` add
`android:usesCleartextTraffic="true"` to the `<application>` tag:

```xml
<application
    android:usesCleartextTraffic="true"
    ...>
```

### iOS
In `ios/Runner/Info.plist` add:

```xml
<key>NSAppTransportSecurity</key>
<dict>
    <key>NSAllowsArbitraryLoads</key>
    <true/>
</dict>
```

## 4. Run

```sh
# Make sure the Rust API server is running first:
ENTSOE_API_KEY=your_key cargo run   # in the educk-rs directory

# Then run the app:
flutter run
```

## What you'll see

- **Three summary cards** at the top: current surplus/deficit, peak surplus time, and % of load covered by renewables right now.
- **Line chart** for the selected time window (default 24h):
  - Green line — Wind + Solar generation
  - Blue line  — Total load / demand
  - Orange line — Surplus (generation − load)
  - Green fill  — Positive surplus zone (renewables exceed demand)
  - Red fill    — Deficit zone (demand exceeds renewables)
  - Orange dashed line — current time ("Now")
  - Grey dashed line  — zero reference
- **Tap / hover** anywhere on the chart for an exact tooltip.
- **Swipe down** to refresh.
- **Hours picker** (top-right) switches between 6h / 12h / 24h / 48h views.
- **Country dropdown** switches bidding zone (populated from the API).
