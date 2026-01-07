use crate::entsoe::EntsoeClient;
use anyhow::Result;

mod entsoe;

#[tokio::main]
async fn main() -> Result<()> {
    let api_key = std::env::var("ENTSOE_API_KEY")
        .expect("ENTSOE_API_KEY environment variable not set");

    let client = EntsoeClient::new(api_key);

    // Example 1: Day-ahead total load forecast for Czech Republic
    println!("=== Day-Ahead Total Load Forecast (A65) ===");
    println!("Fetching data for Czech Republic...\n");

    let load_forecast = client
        .fetch_day_ahead_total_load_forecast(
            "10YCZ-CEPS-----N",
            "202308140000",
            "202308170000",
        )
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
        println!("    First 5 points with timestamps:");

        let timestamped = series.period.timestamped_points()?;
        for point in timestamped.iter().take(5) {
            println!(
                "      {} (pos {}): {:.2} MW",
                point.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
                point.position,
                point.quantity
            );
        }
    }

    println!("\nStatistics:");
    println!("  Total Forecast: {:.2} MW", load_forecast.total_forecast());
    println!(
        "  Average: {:.2} MW",
        load_forecast.average_forecast()
    );

    if let Some((min_point, max_point)) = load_forecast.min_max_with_time()? {
        println!(
            "  Min: {:.2} MW at {}",
            min_point.quantity,
            min_point.timestamp.format("%Y-%m-%d %H:%M:%S UTC")
        );
        println!(
            "  Max: {:.2} MW at {}",
            max_point.quantity,
            max_point.timestamp.format("%Y-%m-%d %H:%M:%S UTC")
        );
    }

    // Example 2: Day-ahead generation forecast for Belgium
    println!("\n\n=== Day-Ahead Generation Forecast (A71) ===");
    println!("Fetching data for Belgium...\n");

    let gen_forecast = client
        .fetch_day_ahead_generation_forecast(
            "10YBE----------2",
            "202308152200",
            "202308162200",
        )
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
        println!("    First 5 points with timestamps:");

        let timestamped = series.period.timestamped_points()?;
        for point in timestamped.iter().take(5) {
            println!(
                "      {} (pos {}): {:.2} MW",
                point.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
                point.position,
                point.quantity
            );
        }
    }

    println!("\nStatistics:");
    println!("  Total Forecast: {:.2} MW", gen_forecast.total_forecast());
    println!("  Average: {:.2} MW", gen_forecast.average_forecast());

    if let Some((min_point, max_point)) = gen_forecast.min_max_with_time()? {
        println!(
            "  Min: {:.2} MW at {}",
            min_point.quantity,
            min_point.timestamp.format("%Y-%m-%d %H:%M:%S UTC")
        );
        println!(
            "  Max: {:.2} MW at {}",
            max_point.quantity,
            max_point.timestamp.format("%Y-%m-%d %H:%M:%S UTC")
        );
    }

    // Example: Export to CSV with proper timestamps
    println!("\n\n=== Exporting to CSV format ===");
    println!("Timestamp,Position,Load (MW)");
    let timestamped_points = load_forecast.all_timestamped_points()?;
    for point in timestamped_points.iter().take(10) {
        println!(
            "{},{},{:.2}",
            point.timestamp.to_rfc3339(),
            point.position,
            point.quantity
        );
    }

    Ok(())
}