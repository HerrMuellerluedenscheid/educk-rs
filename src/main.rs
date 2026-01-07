pub mod entsoe;
pub mod server;

use crate::entsoe::EntsoeClient;
use crate::entsoe::analysis::RenewableSurplus;
use crate::server::start_server;
use anyhow::Result;
use plotly::{
    Plot, Scatter,
    common::{Mode, Title},
    layout::{Axis, Layout},
};

fn plot_renewable_surplus(surplus_series: &[RenewableSurplus]) {
    // Extract data
    let timestamps: Vec<String> = surplus_series
        .iter()
        .map(|s| s.timestamp.format("%Y-%m-%d %H:%M").to_string())
        .collect();

    let generation: Vec<f64> = surplus_series.iter().map(|s| s.generation).collect();
    let load: Vec<f64> = surplus_series.iter().map(|s| s.load).collect();
    let surplus: Vec<f64> = surplus_series.iter().map(|s| s.surplus).collect();

    // Create traces
    let generation_trace = Scatter::new(timestamps.clone(), generation)
        .name("Wind + Solar Generation")
        .mode(Mode::LinesMarkers)
        .line(plotly::common::Line::new().color("green").width(2.0));

    let load_trace = Scatter::new(timestamps.clone(), load)
        .name("Total Load")
        .mode(Mode::LinesMarkers)
        .line(plotly::common::Line::new().color("blue").width(2.0));

    let surplus_trace = Scatter::new(timestamps, surplus)
        .name("Surplus (Generation - Load)")
        .mode(Mode::LinesMarkers)
        .line(plotly::common::Line::new().color("orange").width(2.0));

    // Create layout
    // let layout = Layout::new()
    //     .title(Title::new("Renewable Energy Forecast"))
    //     .x_axis(Axis::new().title(Title::new("Time")))
    //     .y_axis(Axis::new().title(Title::new("Power (MW)")))
    //     .height(600);

    // Create plot
    let mut plot = Plot::new();
    plot.add_trace(generation_trace);
    plot.add_trace(load_trace);
    plot.add_trace(surplus_trace);
    // plot.set_layout(layout);

    // Show in browser
    plot.show();
}

#[tokio::main]
async fn main() -> Result<()> {
    let api_key =
        std::env::var("ENTSOE_API_KEY").expect("ENTSOE_API_KEY environment variable not set");

    let client = EntsoeClient::new(api_key);

    println!("=== Finding Maximum Renewable Energy Surplus ===\n");

    // Find the peak renewable surplus for Belgium
    let max_surplus = client
        .find_max_renewable_surplus("10YBE----------2", "202308152200", "202308162200")
        .await?;

    println!("Peak Renewable Energy Availability:");
    println!(
        "  Time: {}",
        max_surplus.timestamp.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!("  Generation: {:.2} MW", max_surplus.generation);
    println!("  Load: {:.2} MW", max_surplus.load);
    println!("  Surplus: {:.2} MW", max_surplus.surplus);
    println!("  Surplus %: {:.2}%", max_surplus.surplus_percentage());

    // Get the full time series for analysis
    println!("\n=== Full Renewable Surplus Time Series ===\n");
    let surplus_series = client
        .get_renewable_surplus_series("10YBE----------2", "202308152200", "202308162200")
        .await?;

    println!("Total data points: {}", surplus_series.len());
    println!("\nFirst 10 hours:");
    for surplus in surplus_series.iter().take(10) {
        let indicator = if surplus.has_excess() { "✓" } else { "✗" };
        println!(
            "  {} {} | Gen: {:7.2} MW | Load: {:7.2} MW | Surplus: {:+7.2} MW",
            surplus.timestamp.format("%Y-%m-%d %H:%M"),
            indicator,
            surplus.generation,
            surplus.load,
            surplus.surplus
        );
    }

    // Find periods of high renewable availability
    let high_surplus_periods: Vec<_> = surplus_series
        .iter()
        .filter(|s| s.surplus > 0.0 && s.surplus_percentage() > 10.0)
        .collect();

    println!(
        "\n=== Periods with >10% Renewable Surplus ===\n({} hours)",
        high_surplus_periods.len()
    );
    for surplus in high_surplus_periods.iter().take(5) {
        println!(
            "  {} | Surplus: {:.2} MW ({:.1}%)",
            surplus.timestamp.format("%Y-%m-%d %H:%M"),
            surplus.surplus,
            surplus.surplus_percentage()
        );
    }

    // Export to CSV
    println!("\n=== CSV Export ===");
    println!("Timestamp,Generation (MW),Load (MW),Surplus (MW),Surplus %");
    for surplus in surplus_series.iter().take(24) {
        println!(
            "{},{:.2},{:.2},{:.2},{:.2}",
            surplus.timestamp.to_rfc3339(),
            surplus.generation,
            surplus.load,
            surplus.surplus,
            surplus.surplus_percentage()
        );
    }

    // Generate plot
    println!("\n=== Generating Plot ===");
    println!("Opening plot in browser...");
    plot_renewable_surplus(&surplus_series);

    start_server().await?;
    Ok(())
}
