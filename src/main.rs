#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use fragile_notepad::{
    app::{self, App},
    ipc::{self, SingleInstanceConfig},
    startup,
};

fn main() -> iced::Result {
    let mut options = match startup::parse_arguments(
        std::env::args_os().skip(1),
        &std::env::current_dir()
            .map_err(|error| iced::Error::WindowCreationFailed(Box::new(error)))?,
    ) {
        Ok(startup::StartupCommand::Launch(options)) => options,
        Ok(startup::StartupCommand::Help) => {
            attach_parent_console();
            println!("{}", startup::CLI_HELP);
            return Ok(());
        }
        Ok(startup::StartupCommand::Version) => {
            attach_parent_console();
            println!("fragile-notepad {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Err(error) => {
            attach_parent_console();
            return Err(iced::Error::WindowCreationFailed(Box::new(
                std::io::Error::new(std::io::ErrorKind::InvalidInput, error),
            )));
        }
    };
    if startup::startup_probe_enabled() {
        options.restore_session = false;
        startup::mark_startup_started();
    }

    let single_instance = SingleInstanceConfig::new(if startup::startup_probe_enabled() {
        format!("fragile-notepad-startup-probe-{}", std::process::id())
    } else {
        String::from("fragile-notepad")
    });

    match ipc::claim_or_signal_with_files(&single_instance, &options.files).map_err(|error| {
        attach_parent_console();
        iced::Error::WindowCreationFailed(Box::new(std::io::Error::new(
            error.kind(),
            format!("single-instance IPC initialization failed: {error}"),
        )))
    })? {
        ipc::Startup::Primary(instance) => app::register_single_instance(instance),
        ipc::Startup::Secondary => return Ok(()),
    }

    app::initialize_platform();
    let ui_scale = startup::ui_scale_factor();

    iced::daemon(
        move || App::new_with_options(options.clone()),
        App::update,
        App::view,
    )
    .settings(startup::iced_settings())
    .subscription(App::subscription)
    .title(App::title)
    .theme(App::theme)
    .scale_factor(move |_, _| ui_scale)
    .run()
}

fn attach_parent_console() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::System::Console::{
            ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
        };
        let output = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
        if output.is_null() || output == INVALID_HANDLE_VALUE {
            unsafe {
                AttachConsole(ATTACH_PARENT_PROCESS);
            }
        }
    }
}
