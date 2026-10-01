use std::process::ExitCode;

use clap::{CommandFactory, Parser, error::ErrorKind};
use ecgdisp::app::{self, EcgApp};
use ecgdisp::{cli, leads, nav::RecordList};
use eframe::{egui_wgpu::WgpuSetup, wgpu};

/// By default wgpu also probes OpenGL. Under WSLg that makes Mesa print
/// EGL/ZINK warnings even though a Vulkan adapter is chosen in the end, so
/// only probe the native backends (Vulkan/DX12/Metal) unless `WGPU_BACKEND`
/// asks for something else (e.g. `WGPU_BACKEND=gl` on a machine without Vulkan).
fn wgpu_options() -> eframe::WgpuConfiguration {
    let mut config = eframe::WgpuConfiguration::default();
    if let WgpuSetup::CreateNew(setup) = &mut config.wgpu_setup
        && wgpu::Backends::from_env().is_none()
    {
        setup.instance_descriptor.backends = wgpu::Backends::PRIMARY;
    }
    config
}

fn main() -> ExitCode {
    let args = cli::Args::parse();
    let wanted = leads::resolve(&args.leads).unwrap_or_else(|e| {
        cli::Args::command()
            .error(ErrorKind::InvalidValue, e)
            .exit()
    });
    let hea = cli::record_header_path(&args.record, &args.dir).unwrap_or_else(|e| {
        cli::Args::command()
            .error(ErrorKind::InvalidValue, e)
            .exit()
    });

    let chart = match app::load_chart(&hea, &wanted) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ecgdisp: cannot load record: {e}");
            return ExitCode::FAILURE;
        }
    };

    let title = app::window_title(&chart);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([1600.0, 950.0]),
        wgpu_options: wgpu_options(),
        ..Default::default()
    };
    let app = EcgApp::new(chart, RecordList::scan(&hea), wanted);
    match eframe::run_native("ecgdisp", options, Box::new(|_cc| Ok(Box::new(app)))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ecgdisp: {e}");
            ExitCode::FAILURE
        }
    }
}
