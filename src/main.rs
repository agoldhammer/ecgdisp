use std::process::ExitCode;

use clap::{CommandFactory, Parser, error::ErrorKind};
use ecgdisp::{app::EcgApp, cli, leads, wfdb};

fn main() -> ExitCode {
    let args = cli::Args::parse();
    let wanted = leads::resolve(&args.leads).unwrap_or_else(|e| {
        cli::Args::command()
            .error(ErrorKind::InvalidValue, e)
            .exit()
    });
    let hea = cli::record_header_path(&args.record, &args.dir);

    let record = match wfdb::load_record(&hea) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("ecgdisp: cannot load record: {e}");
            return ExitCode::FAILURE;
        }
    };
    let traces = match leads::select(&record, &wanted) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("ecgdisp: {e}");
            return ExitCode::FAILURE;
        }
    };

    let title = format!("ecgdisp — {}", record.name);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([1600.0, 950.0]),
        ..Default::default()
    };
    let app = EcgApp::new(&record, traces);
    match eframe::run_native(&title, options, Box::new(|_cc| Ok(Box::new(app)))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ecgdisp: {e}");
            ExitCode::FAILURE
        }
    }
}
