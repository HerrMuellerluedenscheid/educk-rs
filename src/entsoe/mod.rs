mod areas;

use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;

const BASE_URL: &str = "https://web-api.tp.entsoe.eu/api";

#[derive(Error, Debug)]
pub enum EntsoeError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("XML parsing failed: {0}")]
    XmlParsing(#[from] quick_xml::DeError),
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

// Main response structure
#[derive(Debug, Deserialize)]
#[serde(rename = "GL_MarketDocument")]
pub struct GlMarketDocument {
    #[serde(rename = "mRID")]
    pub mrid: String,
    #[serde(rename = "revisionNumber")]
    pub revision_number: String,
    #[serde(rename = "type")]
    pub doc_type: String,
    #[serde(rename = "process.processType")]
    pub process_type: String,
    #[serde(rename = "sender_MarketParticipant.mRID")]
    pub sender_mrid: ParticipantId,
    #[serde(rename = "sender_MarketParticipant.marketRole.type")]
    pub sender_role: String,
    #[serde(rename = "receiver_MarketParticipant.mRID")]
    pub receiver_mrid: ParticipantId,
    #[serde(rename = "receiver_MarketParticipant.marketRole.type")]
    pub receiver_role: String,
    #[serde(rename = "createdDateTime")]
    pub created_date_time: String,
    #[serde(rename = "time_Period.timeInterval")]
    pub time_period_interval: TimeInterval,
    #[serde(rename = "TimeSeries")]
    pub time_series: Vec<TimeSeries>,
}

#[derive(Debug, Deserialize)]
pub struct ParticipantId {
    #[serde(rename = "$value")]
    pub value: String,
    #[serde(rename = "@codingScheme")]
    pub coding_scheme: String,
}

#[derive(Debug, Deserialize)]
pub struct TimeInterval {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Deserialize)]
pub struct TimeSeries {
    #[serde(rename = "mRID")]
    pub mrid: String,
    #[serde(rename = "businessType")]
    pub business_type: String,
    #[serde(rename = "objectAggregation")]
    pub object_aggregation: String,
    #[serde(rename = "outBiddingZone_Domain.mRID")]
    pub out_bidding_zone: Option<AreaId>,
    #[serde(rename = "inBiddingZone_Domain.mRID")]
    pub in_bidding_zone: Option<AreaId>,
    #[serde(rename = "quantity_Measure_Unit.name")]
    pub quantity_measure_unit: String,
    #[serde(rename = "curveType")]
    pub curve_type: String,
    #[serde(rename = "Period")]
    pub period: Period,
}

#[derive(Debug, Deserialize)]
pub struct AreaId {
    #[serde(rename = "$value")]
    pub value: String,
    #[serde(rename = "@codingScheme")]
    pub coding_scheme: String,
}

#[derive(Debug, Deserialize)]
pub struct Period {
    #[serde(rename = "timeInterval")]
    pub time_interval: TimeInterval,
    pub resolution: String,
    #[serde(rename = "Point")]
    pub points: Vec<Point>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Point {
    pub position: u32,
    pub quantity: f64,
}

pub struct EntsoeClient {
    client: Client,
    api_key: String,
}

impl EntsoeClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.into(),
        }
    }

    /// Fetch day-ahead total load forecast (A65)
    /// Example: Czech Republic bidding zone "10YCZ-CEPS-----N"
    pub async fn fetch_day_ahead_total_load_forecast(
        &self,
        out_bidding_zone: &str,
        period_start: &str,
        period_end: &str,
    ) -> Result<GlMarketDocument, EntsoeError> {
        let url = format!(
            "{}?securityToken={}&documentType=A65&processType=A01&outBiddingZone_Domain={}&periodStart={}&periodEnd={}",
            BASE_URL, self.api_key, out_bidding_zone, period_start, period_end
        );

        self.fetch_and_parse(&url).await
    }

    /// Fetch day-ahead generation forecast (A71)
    /// Example: Belgium domain "10YBE----------2"
    pub async fn fetch_day_ahead_generation_forecast(
        &self,
        in_domain: &str,
        period_start: &str,
        period_end: &str,
    ) -> Result<GlMarketDocument, EntsoeError> {
        let url = format!(
            "{}?securityToken={}&documentType=A71&processType=A01&in_Domain={}&periodStart={}&periodEnd={}",
            BASE_URL, self.api_key, in_domain, period_start, period_end
        );

        self.fetch_and_parse(&url).await
    }

    async fn fetch_and_parse(&self, url: &str) -> Result<GlMarketDocument, EntsoeError> {
        let xml = self.client.get(url).send().await?.text().await?;

        // Check for error response
        if xml.contains("<Reason>") || xml.contains("<code>") {
            return Err(EntsoeError::InvalidResponse(xml));
        }

        let document: GlMarketDocument = quick_xml::de::from_str(&xml).map_err(|e| {
            eprintln!("Failed to parse XML: {}", e);
            eprintln!("XML content: {}", xml);
            e
        })?;

        Ok(document)
    }
}

// Helper functions to work with the data
impl GlMarketDocument {
    /// Get all time series points with their timestamps
    pub fn all_points_with_time(&self) -> Vec<(String, u32, f64)> {
        let mut result = Vec::new();

        for series in &self.time_series {
            let start = &series.period.time_interval.start;
            for point in &series.period.points {
                result.push((start.clone(), point.position, point.quantity));
            }
        }

        result
    }

    /// Get all points flattened (timestamp, quantity)
    pub fn all_points(&self) -> Vec<(String, f64)> {
        self.all_points_with_time()
            .into_iter()
            .map(|(time, _pos, qty)| (time, qty))
            .collect()
    }

    /// Get total forecast across all points
    pub fn total_forecast(&self) -> f64 {
        self.time_series
            .iter()
            .map(|ts| ts.period.points.iter().map(|p| p.quantity).sum::<f64>())
            .sum()
    }

    /// Get average forecast value
    pub fn average_forecast(&self) -> f64 {
        let total_points: usize = self
            .time_series
            .iter()
            .map(|ts| ts.period.points.len())
            .sum();

        if total_points == 0 {
            return 0.0;
        }

        self.total_forecast() / total_points as f64
    }

    /// Get min and max values
    pub fn min_max(&self) -> Option<(f64, f64)> {
        let values: Vec<f64> = self
            .time_series
            .iter()
            .flat_map(|ts| ts.period.points.iter().map(|p| p.quantity))
            .collect();

        if values.is_empty() {
            return None;
        }

        Some((
            values.iter().cloned().fold(f64::INFINITY, f64::min),
            values.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_parse_load_forecast_xml() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<GL_MarketDocument xmlns="urn:iec62325.351:tc57wg16:451-6:generationloaddocument:3:0">
    <mRID>test123</mRID>
    <revisionNumber>1</revisionNumber>
    <type>A65</type>
    <process.processType>A01</process.processType>
    <sender_MarketParticipant.mRID codingScheme="A01">10X1001A1001A450</sender_MarketParticipant.mRID>
    <sender_MarketParticipant.marketRole.type>A32</sender_MarketParticipant.marketRole.type>
    <receiver_MarketParticipant.mRID codingScheme="A01">10X1001A1001A450</receiver_MarketParticipant.mRID>
    <receiver_MarketParticipant.marketRole.type>A33</receiver_MarketParticipant.marketRole.type>
    <createdDateTime>2026-01-07T19:26:41Z</createdDateTime>
    <time_Period.timeInterval>
        <start>2023-08-14T00:00Z</start>
        <end>2023-08-17T00:00Z</end>
    </time_Period.timeInterval>
    <TimeSeries>
        <mRID>1</mRID>
        <businessType>A04</businessType>
        <objectAggregation>A01</objectAggregation>
        <outBiddingZone_Domain.mRID codingScheme="A01">10YCZ-CEPS-----N</outBiddingZone_Domain.mRID>
        <quantity_Measure_Unit.name>MAW</quantity_Measure_Unit.name>
        <curveType>A03</curveType>
        <Period>
            <timeInterval>
                <start>2023-08-14T00:00Z</start>
                <end>2023-08-17T00:00Z</end>
            </timeInterval>
            <resolution>PT60M</resolution>
            <Point>
                <position>1</position>
                <quantity>4933</quantity>
            </Point>
        </Period>
    </TimeSeries>
</GL_MarketDocument>"#;

        let doc: GlMarketDocument = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(doc.mrid, "test123");
        assert_eq!(doc.doc_type, "A65");
        assert_eq!(doc.time_series.len(), 1);
        assert_eq!(doc.time_series[0].period.points.len(), 1);
        assert_eq!(doc.time_series[0].period.points[0].quantity, 4933.0);
    }

    #[tokio::test]
    async fn test_fetch_load_forecast() {
        let api_key = match std::env::var("ENTSOE_API_KEY") {
            Ok(key) => key,
            Err(_) => {
                eprintln!("no api key");
                return;
            } // Skip test if no API key
        };

        let client = EntsoeClient::new(api_key);

        let result = client
            .fetch_day_ahead_total_load_forecast("10YCZ-CEPS-----N", "202601070000", "202601080000")
            .await;

        assert!(result.is_ok());
        let doc = result.unwrap();
        assert!(!doc.time_series.is_empty());
        assert!(doc.total_forecast() > 0.0);
        println!("{:?}", doc.time_series);
    }
}
