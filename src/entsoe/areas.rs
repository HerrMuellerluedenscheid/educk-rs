use std::collections::HashMap;
use once_cell::sync::Lazy;

/// ISO 3166-1 alpha-2 country code
pub type CountryCode = &'static str;

/// ENTSO-E area/bidding zone code
pub type AreaCode = &'static str;

/// Represents an ENTSO-E bidding zone or control area
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiddingZone {
    pub code: AreaCode,
    pub country_code: CountryCode,
    pub name: &'static str,
    pub tso: Option<&'static str>, // Transmission System Operator
}

impl BiddingZone {
    pub const fn new(
        code: AreaCode,
        country_code: CountryCode,
        name: &'static str,
        tso: Option<&'static str>,
    ) -> Self {
        Self {
            code,
            country_code,
            name,
            tso,
        }
    }
}

/// All available ENTSO-E bidding zones
pub static BIDDING_ZONES: Lazy<HashMap<CountryCode, Vec<BiddingZone>>> = Lazy::new(|| {
    let zones = vec![
        BiddingZone::new("10YAL-KESH-----5", "AL", "Albania", None),
        BiddingZone::new("10YAT-APG------L", "AT", "Austria", None),
        BiddingZone::new("10Y1001A1001A51S", "BY", "Belarus", None),
        BiddingZone::new("10YBE----------2", "BE", "Belgium", None),
        BiddingZone::new("10YBA-JPCC-----D", "BA", "Bosnia and Herzegovina", None),
        BiddingZone::new("10YCA-BULGARIA-R", "BG", "Bulgaria", None),
        BiddingZone::new("10YHR-HEP------M", "HR", "Croatia", None),
        BiddingZone::new("10YCY-1001A0003J", "CY", "Cyprus", None),
        BiddingZone::new("10YCZ-CEPS-----N", "CZ", "Czech Republic", None),
        BiddingZone::new("10Y1001A1001A796", "DK", "Denmark", None),
        BiddingZone::new("10Y1001A1001A39I", "EE", "Estonia", None),
        BiddingZone::new("10YFI-1--------U", "FI", "Finland", None),
        BiddingZone::new("10YFR-RTE------C", "FR", "France", None),
        BiddingZone::new("10Y1001A1001A83F", "DE", "Germany", None),
        BiddingZone::new("10YDE-VE-------2", "DE", "Germany", Some("50Hertz")),
        BiddingZone::new("10YDE-RWENET---I", "DE", "Germany", Some("Amprion")),
        BiddingZone::new("10YDE-EON------1", "DE", "Germany", Some("TenneT")),
        BiddingZone::new("10YDE-ENBW-----N", "DE", "Germany", Some("TransnetBW")),
        BiddingZone::new("10YGR-HTSO-----Y", "GR", "Greece", None),
        BiddingZone::new("10YHU-MAVIR----U", "HU", "Hungary", None),
        BiddingZone::new("IS", "IS", "Iceland", None),
        BiddingZone::new("10YIE-1001A00010", "IE", "Ireland", None),
        BiddingZone::new("10Y1001A1001A016", "GB", "Northern Ireland", None),
        BiddingZone::new("10YIT-GRTN-----B", "IT", "Italy", None),
        BiddingZone::new("10Y1001A1001A885", "IT", "Italy", Some("Saco AC")),
        BiddingZone::new("10Y1001A1001A893", "IT", "Italy", Some("Saco DC")),
        BiddingZone::new("10Y1001A1001A50U", "RU", "Kaliningrad", None),
        BiddingZone::new("10YLV-1001A00074", "LV", "Latvia", None),
        BiddingZone::new("10YLT-1001A0008Q", "LT", "Lithuania", None),
        BiddingZone::new("10YLU-CEGEDEL-NQ", "LU", "Luxembourg", None),
        BiddingZone::new("10YMK-MEPSO----8", "MK", "North Macedonia", None),
        BiddingZone::new("10Y1001A1001A93C", "MT", "Malta", None),
        BiddingZone::new("10Y1001A1001A990", "MD", "Moldova", None),
        BiddingZone::new("10YCS-CG-TSO---S", "ME", "Montenegro", None),
        BiddingZone::new("10YNL----------L", "NL", "Netherlands", None),
        BiddingZone::new("10YNO-0--------C", "NO", "Norway", None),
        BiddingZone::new("10YPL-AREA-----S", "PL", "Poland", None),
        BiddingZone::new("10YPT-REN------W", "PT", "Portugal", None),
        BiddingZone::new("10YRO-TEL------P", "RO", "Romania", None),
        BiddingZone::new("10Y1001A1001A49F", "RU", "Russia", None),
        BiddingZone::new("10YCS-SERBIATSOV", "RS", "Serbia", None),
        BiddingZone::new("10YSK-SEPS-----K", "SK", "Slovakia", None),
        BiddingZone::new("10YSI-ELES-----O", "SI", "Slovenia", None),
        BiddingZone::new("10YES-REE------0", "ES", "Spain", None),
        BiddingZone::new("10YSE-1--------K", "SE", "Sweden", None),
        BiddingZone::new("10YCH-SWISSGRIDZ", "CH", "Switzerland", None),
        BiddingZone::new("10YTR-TEIAS----W", "TR", "Turkey", None),
        BiddingZone::new("10Y1001C--00003F", "UA", "Ukraine", None),
    ];

    // Group by country code
    let mut map: HashMap<CountryCode, Vec<BiddingZone>> = HashMap::new();
    for zone in zones {
        map.entry(zone.country_code)
            .or_insert_with(Vec::new)
            .push(zone);
    }
    map
});

/// Get all bidding zones for a country
pub fn get_zones_by_country(country_code: &str) -> Option<&'static Vec<BiddingZone>> {
    BIDDING_ZONES.get(country_code)
}

/// Get a specific bidding zone by its ENTSO-E code
pub fn get_zone_by_code(area_code: AreaCode) -> Option<&'static BiddingZone> {
    BIDDING_ZONES
        .values()
        .flatten()
        .find(|zone| zone.code == area_code)
}

/// Get the primary bidding zone for a country (first one if multiple exist)
pub fn get_primary_zone(country_code: &str) -> Option<&'static BiddingZone> {
    BIDDING_ZONES.get(country_code).and_then(|zones| zones.first())
}

/// List all available country codes
pub fn list_countries() -> Vec<CountryCode> {
    let mut countries: Vec<_> = BIDDING_ZONES.keys().copied().collect();
    countries.sort();
    countries
}

impl std::fmt::Display for BiddingZone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.tso {
            Some(tso) => write!(f, "{} ({}) - {}", self.name, self.country_code, tso),
            None => write!(f, "{} ({})", self.name, self.country_code),
        }
    }
}