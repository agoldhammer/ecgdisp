use std::process::ExitCode;

use clap::{CommandFactory, Parser, error::ErrorKind};
use ecgdisp::app::{self, EcgApp};
use ecgdisp::{cli, database, leads, nav::RecordList};
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

/// Under WSLg, winit's Wayland backend draws client-side decorations whose
/// drop shadow is left behind on the Windows desktop when the window is
/// maximized. Through X11 (XWayland) WSLg gives the window a native frame
/// instead, so use X11 when running in WSL with an X display available.
fn prefer_x11(kernel_release: &str, has_x_display: bool) -> bool {
    has_x_display && kernel_release.to_ascii_lowercase().contains("microsoft")
}

#[cfg(target_os = "linux")]
fn event_loop_builder() -> Option<eframe::EventLoopBuilderHook> {
    use winit::platform::x11::EventLoopBuilderExtX11;
    let release = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let has_x_display = std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty());
    prefer_x11(&release, has_x_display).then(|| {
        Box::new(
            |builder: &mut eframe::EventLoopBuilder<eframe::UserEvent>| {
                builder.with_x11();
            },
        ) as eframe::EventLoopBuilderHook
    })
}

#[cfg(not(target_os = "linux"))]
fn event_loop_builder() -> Option<eframe::EventLoopBuilderHook> {
    None
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

    let db = match database::find(&hea) {
        Some(path) => database::Database::open(&path)
            .inspect_err(|e| eprintln!("ecgdisp: cannot read record database: {e}"))
            .ok(),
        None => {
            eprintln!(
                "ecgdisp: {} not found above {}",
                database::FILE_NAME,
                hea.display()
            );
            None
        }
    };

    let title = app::window_title(&chart);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([1600.0, 950.0]),
        wgpu_options: wgpu_options(),
        event_loop_builder: event_loop_builder(),
        ..Default::default()
    };
    let app = EcgApp::new(chart, RecordList::scan(&hea), wanted, db);
    match eframe::run_native("ecgdisp", options, Box::new(|_cc| Ok(Box::new(app)))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ecgdisp: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::prefer_x11;

    #[test]
    fn x11_is_preferred_only_in_wsl_with_a_display() {
        let wsl = "6.18.35.2-microsoft-standard-WSL2\n";
        assert!(prefer_x11(wsl, true));
        assert!(prefer_x11("4.4.0-19041-Microsoft", true));
        assert!(!prefer_x11(wsl, false));
        assert!(!prefer_x11("6.8.0-45-generic", true));
        assert!(!prefer_x11("", true));
    }
}
