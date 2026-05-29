use once_cell::sync::Lazy;
use std::collections::HashMap;

pub struct CloudRegion {
    pub provider: &'static str,
    pub region: &'static str,
    pub country_code: &'static str,
    pub location: &'static str,
}

/// Keyed by "provider/region" (lowercase).
static CLOUD_REGIONS: Lazy<HashMap<&'static str, CloudRegion>> = Lazy::new(|| {
    let mut m = HashMap::new();

    macro_rules! r {
        ($key:expr, $provider:expr, $region:expr, $cc:expr, $loc:expr) => {
            m.insert(
                $key,
                CloudRegion {
                    provider: $provider,
                    region: $region,
                    country_code: $cc,
                    location: $loc,
                },
            );
        };
    }

    // ── AWS ──────────────────────────────────────────────────────────────────
    r!(
        "aws/eu-central-1",
        "aws",
        "eu-central-1",
        "DE",
        "Frankfurt, Germany"
    );
    r!(
        "aws/eu-central-2",
        "aws",
        "eu-central-2",
        "CH",
        "Zurich, Switzerland"
    );
    r!("aws/eu-west-1", "aws", "eu-west-1", "IE", "Dublin, Ireland");
    r!("aws/eu-west-2", "aws", "eu-west-2", "GB", "London, UK");
    r!("aws/eu-west-3", "aws", "eu-west-3", "FR", "Paris, France");
    r!(
        "aws/eu-north-1",
        "aws",
        "eu-north-1",
        "SE",
        "Stockholm, Sweden"
    );
    r!("aws/eu-south-1", "aws", "eu-south-1", "IT", "Milan, Italy");
    r!(
        "aws/eu-south-2",
        "aws",
        "eu-south-2",
        "ES",
        "Zaragoza, Spain"
    );

    // ── Azure ─────────────────────────────────────────────────────────────────
    r!(
        "azure/germanywestcentral",
        "azure",
        "germanywestcentral",
        "DE",
        "Frankfurt, Germany"
    );
    r!(
        "azure/germanynorth",
        "azure",
        "germanynorth",
        "DE",
        "Berlin, Germany"
    );
    r!(
        "azure/northeurope",
        "azure",
        "northeurope",
        "IE",
        "Dublin, Ireland"
    );
    r!(
        "azure/westeurope",
        "azure",
        "westeurope",
        "NL",
        "Amsterdam, Netherlands"
    );
    r!("azure/uksouth", "azure", "uksouth", "GB", "London, UK");
    r!("azure/ukwest", "azure", "ukwest", "GB", "Cardiff, UK");
    r!(
        "azure/francecentral",
        "azure",
        "francecentral",
        "FR",
        "Paris, France"
    );
    r!(
        "azure/francesouth",
        "azure",
        "francesouth",
        "FR",
        "Marseille, France"
    );
    r!(
        "azure/swedencentral",
        "azure",
        "swedencentral",
        "SE",
        "Gävle, Sweden"
    );
    r!(
        "azure/switzerlandnorth",
        "azure",
        "switzerlandnorth",
        "CH",
        "Zurich, Switzerland"
    );
    r!(
        "azure/switzerlandwest",
        "azure",
        "switzerlandwest",
        "CH",
        "Geneva, Switzerland"
    );
    r!(
        "azure/italynorth",
        "azure",
        "italynorth",
        "IT",
        "Milan, Italy"
    );
    r!(
        "azure/norwayeast",
        "azure",
        "norwayeast",
        "NO",
        "Oslo, Norway"
    );
    r!(
        "azure/norwaywest",
        "azure",
        "norwaywest",
        "NO",
        "Stavanger, Norway"
    );
    r!(
        "azure/polandcentral",
        "azure",
        "polandcentral",
        "PL",
        "Warsaw, Poland"
    );
    r!(
        "azure/spaincentral",
        "azure",
        "spaincentral",
        "ES",
        "Madrid, Spain"
    );
    r!(
        "azure/finlandcentral",
        "azure",
        "finlandcentral",
        "FI",
        "Helsinki, Finland"
    );
    r!(
        "azure/austriaeast",
        "azure",
        "austriaeast",
        "AT",
        "Vienna, Austria"
    );

    // ── GCP ───────────────────────────────────────────────────────────────────
    r!(
        "gcp/europe-west1",
        "gcp",
        "europe-west1",
        "BE",
        "St. Ghislain, Belgium"
    );
    r!(
        "gcp/europe-west2",
        "gcp",
        "europe-west2",
        "GB",
        "London, UK"
    );
    r!(
        "gcp/europe-west3",
        "gcp",
        "europe-west3",
        "DE",
        "Frankfurt, Germany"
    );
    r!(
        "gcp/europe-west4",
        "gcp",
        "europe-west4",
        "NL",
        "Eemshaven, Netherlands"
    );
    r!(
        "gcp/europe-west6",
        "gcp",
        "europe-west6",
        "CH",
        "Zurich, Switzerland"
    );
    r!(
        "gcp/europe-west8",
        "gcp",
        "europe-west8",
        "IT",
        "Milan, Italy"
    );
    r!(
        "gcp/europe-west9",
        "gcp",
        "europe-west9",
        "FR",
        "Paris, France"
    );
    r!(
        "gcp/europe-west10",
        "gcp",
        "europe-west10",
        "DE",
        "Berlin, Germany"
    );
    r!(
        "gcp/europe-west12",
        "gcp",
        "europe-west12",
        "IT",
        "Turin, Italy"
    );
    r!(
        "gcp/europe-north1",
        "gcp",
        "europe-north1",
        "FI",
        "Hamina, Finland"
    );
    r!(
        "gcp/europe-central2",
        "gcp",
        "europe-central2",
        "PL",
        "Warsaw, Poland"
    );
    r!(
        "gcp/europe-southwest1",
        "gcp",
        "europe-southwest1",
        "ES",
        "Madrid, Spain"
    );

    m
});

pub fn lookup(provider: &str, region: &str) -> Option<&'static CloudRegion> {
    let key = format!("{}/{}", provider.to_lowercase(), region.to_lowercase());
    CLOUD_REGIONS.get(key.as_str())
}

pub fn all_regions() -> Vec<&'static CloudRegion> {
    let mut regions: Vec<&'static CloudRegion> = CLOUD_REGIONS.values().collect();
    regions.sort_by_key(|r| (r.provider, r.region));
    regions
}
