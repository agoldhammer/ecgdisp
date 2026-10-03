use std::process::ExitCode;

use clap::{CommandFactory, Parser, error::ErrorKind};
use ecgdisp::app::{self, EcgApp};
use ecgdisp::{archive, cli, database, leads, nav::RecordList};
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

/// Under WSLg, winit's Wayland frame (client-side decorations) leaves its drop
/// shadow behind on the Windows desktop when the window is maximized, and
/// X11 windows get no mouse pointer over their contents. So in WSL the window
/// stays on Wayland but without winit's frame, and the app draws its own.
fn is_wsl(kernel_release: &str) -> bool {
    kernel_release.to_ascii_lowercase().contains("microsoft")
}

fn running_in_wsl() -> bool {
    std::fs::read_to_string("/proc/sys/kernel/osrelease").is_ok_and(|r| is_wsl(&r))
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

    if let Err(e) = archive::ensure_record(&args.zip, &args.dir, &hea) {
        eprintln!("ecgdisp: cannot unpack record folder: {e}");
        return ExitCode::FAILURE;
    }

    let chart = match app::load_chart(&hea, &wanted) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ecgdisp: cannot load record: {e}");
            return ExitCode::FAILURE;
        }
    };

    let db_path = database::find(&hea).or_else(|| args.db.is_file().then(|| args.db.clone()));
    let db = match db_path {
        Some(path) => database::Database::open(&path)
            .inspect_err(|e| eprintln!("ecgdisp: cannot read record database: {e}"))
            .ok(),
        None => {
            eprintln!(
                "ecgdisp: {} not found above {} nor at {}",
                database::FILE_NAME,
                hea.display(),
                args.db.display()
            );
            None
        }
    };

    let custom_frame = running_in_wsl();
    let title = app::window_title(&chart);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([1600.0, 950.0])
            .with_decorations(!custom_frame),
        wgpu_options: wgpu_options(),
        ..Default::default()
    };
    let app = EcgApp::new(chart, RecordList::scan(&hea), wanted, db, custom_frame);
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
    use super::is_wsl;

    #[test]
    fn wsl_is_detected_from_the_kernel_release() {
        assert!(is_wsl("6.18.35.2-microsoft-standard-WSL2\n"));
        assert!(is_wsl("4.4.0-19041-Microsoft"));
        assert!(!is_wsl("6.8.0-45-generic"));
        assert!(!is_wsl(""));
    }
}
