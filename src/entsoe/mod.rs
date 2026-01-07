pub(crate) mod analysis;
pub(crate) mod areas;

use std::collections::HashMap;
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
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
    #[error("Invalid resolution format: {0}")]
    InvalidResolution(String),
    #[error("Invalid timestamp: {0}")]
    InvalidTimestamp(String),
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

#[derive(Debug, Deserialize, Clone)]
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

#[derive(Debug, Deserialize, Clone)]
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

/// Represents a time series point with its actual timestamp
#[derive(Debug, Clone)]
pub struct TimestampedPoint {
    pub timestamp: DateTime<Utc>,
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

    /// Fetch day-ahead generation solar/wind forecast (A69)
    /// Example: Belgium domain "10YBE----------2"
    pub async fn fetch_day_ahead_generation_forecast(
        &self,
        in_domain: &str,
        period_start: &str,
        period_end: &str,
    ) -> Result<GlMarketDocument, EntsoeError> {
        let url = format!(
            "{}?securityToken={}&documentType=A69&processType=A01&in_Domain={}&periodStart={}&periodEnd={}",
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

/// Parse ISO 8601 duration format (PT15M, PT30M, PT60M, etc.)
fn parse_resolution(resolution: &str) -> Result<Duration, EntsoeError> {
    // Format: PT[n]M where n is minutes
    if !resolution.starts_with("PT") || !resolution.ends_with("M") {
        return Err(EntsoeError::InvalidResolution(resolution.to_string()));
    }

    let minutes_str = &resolution[2..resolution.len() - 1];
    let minutes: i64 = minutes_str
        .parse()
        .map_err(|_| EntsoeError::InvalidResolution(resolution.to_string()))?;

    Ok(Duration::minutes(minutes))
}

fn parse_timestamp(timestamp: &str) -> Result<DateTime<Utc>, EntsoeError> {
    let normalized = if timestamp.len() == 17 && timestamp.ends_with('Z') {
        let mut s = timestamp.to_string();
        s.insert_str(16, ":00"); // add seconds
        s
    } else {
        timestamp.to_string()
    };

    DateTime::parse_from_rfc3339(&normalized)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            eprintln!("Failed to parse timestamp: {}", e);
            EntsoeError::InvalidTimestamp(timestamp.to_string())
        })
}

impl Period {
    /// Get all points with their actual timestamps based on resolution
    pub fn timestamped_points(&self) -> Result<Vec<TimestampedPoint>, EntsoeError> {
        let start_time = parse_timestamp(&self.time_interval.start)?;
        let resolution_duration = parse_resolution(&self.resolution)?;

        let timestamped = self
            .points
            .iter()
            .map(|point| {
                // Position starts at 1, so subtract 1 to get offset
                let offset = resolution_duration * (point.position as i32 - 1);
                TimestampedPoint {
                    timestamp: start_time + offset,
                    position: point.position,
                    quantity: point.quantity,
                }
            })
            .collect();

        Ok(timestamped)
    }
}

// Helper functions to work with the data
impl GlMarketDocument {
    /// Get all timestamped points across all time series
    pub fn all_timestamped_points(&self) -> Result<Vec<TimestampedPoint>, EntsoeError> {
        let mut timestamp_map: HashMap<DateTime<Utc>, f64> = HashMap::new();

        // Aggregate all points by timestamp
        for series in &self.time_series {
            let points = series.period.timestamped_points()?;
            for point in points {
                *timestamp_map.entry(point.timestamp).or_insert(0.0) += point.quantity;
            }
        }

        // Convert back to Vec and sort by timestamp
        let mut result: Vec<TimestampedPoint> = timestamp_map
            .into_iter()
            .map(|(timestamp, quantity)| TimestampedPoint {
                timestamp,
                position: 0, // Position doesn't make sense for aggregated data
                quantity,
            })
            .collect();

        result.sort_by_key(|p| p.timestamp);

        // Reassign positions based on sorted order
        for (i, point) in result.iter_mut().enumerate() {
            point.position = (i + 1) as u32;
        }

        Ok(result)
    }

    /// Get all points with their timestamps as ISO strings
    pub fn all_points_with_time(&self) -> Result<Vec<(String, u32, f64)>, EntsoeError> {
        let timestamped = self.all_timestamped_points()?;

        Ok(timestamped
            .into_iter()
            .map(|tp| (tp.timestamp.to_rfc3339(), tp.position, tp.quantity))
            .collect())
    }

    /// Get all points flattened (timestamp, quantity)
    pub fn all_points(&self) -> Result<Vec<(DateTime<Utc>, f64)>, EntsoeError> {
        let timestamped = self.all_timestamped_points()?;

        Ok(timestamped
            .into_iter()
            .map(|tp| (tp.timestamp, tp.quantity))
            .collect())
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

    /// Get min and max values with timestamps
    pub fn min_max_with_time(
        &self,
    ) -> Result<Option<(TimestampedPoint, TimestampedPoint)>, EntsoeError> {
        let points = self.all_timestamped_points()?;

        if points.is_empty() {
            return Ok(None);
        }

        let min_point = points
            .iter()
            .min_by(|a, b| a.quantity.partial_cmp(&b.quantity).unwrap())
            .unwrap()
            .clone();

        let max_point = points
            .iter()
            .max_by(|a, b| a.quantity.partial_cmp(&b.quantity).unwrap())
            .unwrap()
            .clone();

        Ok(Some((min_point, max_point)))
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
    use chrono::{Datelike, Timelike};

    #[test]
    fn test_parse_resolution() {
        assert_eq!(parse_resolution("PT15M").unwrap(), Duration::minutes(15));
        assert_eq!(parse_resolution("PT30M").unwrap(), Duration::minutes(30));
        assert_eq!(parse_resolution("PT60M").unwrap(), Duration::minutes(60));
        assert!(parse_resolution("invalid").is_err());
    }

    #[test]
    fn test_parse_timestamp() {
        let ts = parse_timestamp("2023-08-14T22:00Z").unwrap();
        assert_eq!(ts.year(), 2023);
        assert_eq!(ts.month(), 8);
        assert_eq!(ts.day(), 14);
        assert_eq!(ts.hour(), 22);
    }

    #[tokio::test]
    async fn test_timestamped_points() {
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
            <Point>
                <position>2</position>
                <quantity>4832</quantity>
            </Point>
            <Point>
                <position>3</position>
                <quantity>4911</quantity>
            </Point>
        </Period>
    </TimeSeries>
</GL_MarketDocument>"#;

        let doc: GlMarketDocument = quick_xml::de::from_str(xml).unwrap();
        let points = doc.all_timestamped_points().unwrap();

        assert_eq!(points.len(), 3);

        // First point at 2023-08-14T00:00Z
        assert_eq!(points[0].position, 1);
        assert_eq!(
            points[0].timestamp.to_rfc3339(),
            "2023-08-14T00:00:00+00:00"
        );

        // Second point at 2023-08-14T01:00Z (60 minutes later)
        assert_eq!(points[1].position, 2);
        assert_eq!(
            points[1].timestamp.to_rfc3339(),
            "2023-08-14T01:00:00+00:00"
        );

        // Third point at 2023-08-14T02:00Z (120 minutes after start)
        assert_eq!(points[2].position, 3);
        assert_eq!(
            points[2].timestamp.to_rfc3339(),
            "2023-08-14T02:00:00+00:00"
        );
    }
}
