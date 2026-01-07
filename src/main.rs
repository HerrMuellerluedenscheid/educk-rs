use crate::entsoe::EntsoeClient;
use anyhow::Result;

mod entsoe;

#[tokio::main]
async fn main() -> Result<()> {
    let api_key =
        std::env::var("ENTSOE_API_KEY").expect("ENTSOE_API_KEY environment variable not set");

    let client = EntsoeClient::new(api_key);
    let in_domain = "10YCZ-CEPS-----N".to_string();
    let now = chrono::Utc::now();

    let period_start = now.format("%Y%m%d%H%M").to_string();
    let period_end = (now + chrono::Duration::hours(24))
        .format("%Y%m%d%H%M")
        .to_string();
    // Example 1: Day-ahead total load forecast for Czech Republic
    println!("=== Day-Ahead Total Load Forecast (A65) ===");
    println!("Fetching data for Czech Republic...\n");

    let load_forecast = client
        .fetch_day_ahead_total_load_forecast(&in_domain, &period_start, &period_end)
        .await?;

    println!("Document Information:");
    println!("  ID: {}", load_forecast.mrid);
    println!("  Type: {}", load_forecast.doc_type);
    println!("  Created: {}", load_forecast.created_date_time);
    println!(
        "  Period: {} to {}",
        load_forecast.time_period_interval.start, load_forecast.time_period_interval.end
    );

    println!("\nTime Series:");
    for (i, series) in load_forecast.time_series.iter().enumerate() {
        println!("  Series {}:", i + 1);
        println!("    Business Type: {}", series.business_type);
        println!("    Unit: {}", series.quantity_measure_unit);
        println!("    Resolution: {}", series.period.resolution);

        if let Some(zone) = &series.out_bidding_zone {
            println!("    Out Bidding Zone: {}", zone.value);
        }

        println!("    Total Points: {}", series.period.points.len());
        println!("    First 5 points:");
        for point in series.period.points.iter().take(5) {
            println!("      Position {}: {} MW", point.position, point.quantity);
        }
    }

    println!("\nStatistics:");
    println!("  Total Forecast: {:.2} MW", load_forecast.total_forecast());
    println!("  Average: {:.2} MW", load_forecast.average_forecast());
    if let Some((min, max)) = load_forecast.min_max() {
        println!("  Min: {:.2} MW", min);
        println!("  Max: {:.2} MW", max);
    }

    // Example 2: Day-ahead generation forecast for Belgium
    println!("\n\n=== Day-Ahead Generation Forecast (A71) ===");
    println!("Fetching data for Belgium...\n");

    let gen_forecast = client
        .fetch_day_ahead_generation_forecast(&in_domain, &period_start, &period_end)
        .await?;

    println!("Document Information:");
    println!("  ID: {}", gen_forecast.mrid);
    println!("  Type: {}", gen_forecast.doc_type);
    println!("  Created: {}", gen_forecast.created_date_time);
    println!(
        "  Period: {} to {}",
        gen_forecast.time_period_interval.start, gen_forecast.time_period_interval.end
    );

    println!("\nTime Series:");
    for (i, series) in gen_forecast.time_series.iter().enumerate() {
        println!("  Series {}:", i + 1);
        println!("    Business Type: {}", series.business_type);
        println!("    Unit: {}", series.quantity_measure_unit);
        println!("    Resolution: {}", series.period.resolution);

        if let Some(zone) = &series.in_bidding_zone {
            println!("    In Bidding Zone: {}", zone.value);
        }

        println!("    Total Points: {}", series.period.points.len());
        println!("    First 5 points:");
        for point in series.period.points.iter().take(5) {
            println!("      Position {}: {} MW", point.position, point.quantity);
        }
    }

    println!("\nStatistics:");
    println!("  Total Forecast: {:.2} MW", gen_forecast.total_forecast());
    println!("  Average: {:.2} MW", gen_forecast.average_forecast());
    if let Some((min, max)) = gen_forecast.min_max() {
        println!("  Min: {:.2} MW", min);
        println!("  Max: {:.2} MW", max);
    }

    // Example: Export to CSV
    println!("\n\n=== Exporting to CSV format ===");
    println!("Timestamp,Position,Load (MW)");
    for (time, pos, qty) in load_forecast.all_points_with_time().iter().take(10) {
        println!("{},{},{}", time, pos, qty);
    }

    Ok(())
}
