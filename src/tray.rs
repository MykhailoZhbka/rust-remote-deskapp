use crate::client::translate;
#[cfg(windows)]
use crate::ipc::Data;
#[cfg(windows)]
use hbb_common::tokio;
use hbb_common::{
    config::{Config},
    allow_err, log
};
use std::sync::{Arc, Mutex};
#[cfg(windows)]
use std::time::Duration;

pub fn start_tray() {
    allow_err!(make_tray());
}

pub fn make_tray() -> hbb_common::ResultType<()> {
    // https://github.com/tauri-apps/tray-icon/blob/dev/examples/tao.rs
    use hbb_common::anyhow::Context;
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tray_icon::{
        menu::{Menu, MenuEvent, MenuItem},
        TrayIconBuilder, TrayIconEvent as TrayEvent,
    };
    let icon;
    #[cfg(target_os = "macos")]
    {
		//const LIGHT: &[u8] = include_bytes!("../res/mac-tray-light-x2.png");
		//icon = LIGHT;
		icon = include_bytes!("../res/mac-tray-light-x2.png"); // use as template, so color is not important
    }
    #[cfg(not(target_os = "macos"))]
    {
        icon = include_bytes!("../res/tray-icon.ico");
    }
    let (icon_rgba, icon_width, icon_height) = {
        let image = image::load_from_memory(icon)
            .context("Failed to open icon path")?
            .into_rgba8();
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        (rgba, width, height)
    };
    let icon = tray_icon::Icon::from_rgba(icon_rgba, icon_width, icon_height)
        .context("Failed to open icon")?;

    let event_loop = EventLoopBuilder::new().build();

    let tray_menu = Menu::new();
    let quit_i = MenuItem::new(translate("Exit".to_owned()), true, None);
    let open_i = MenuItem::new(translate("Open".to_owned()), true, None);
	//let stopserv_i = MenuItem::new(crate::client::translate("Stop Service".to_owned()), true, None);
	//tray_menu.append_items(&[&open_i, &stopserv_i, &quit_i]);
    tray_menu.append_items(&[&open_i, &quit_i]).ok();
    let tooltip = |count: usize| {
        if count == 0 {
            format!(
                "{} {}",
                crate::get_app_name(),
                translate("Service is running".to_owned()),
            )
        } else {
            format!(
                "{} - {}\n{}",
                crate::get_app_name(),
                translate("Ready".to_owned()),
                translate("{".to_string() + &format!("{count}") + "} sessions"),
            )
        }
    };
    let _tray_icon = Some(
        TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            /*.with_tooltip(format!(
                "{} {}",
                crate::get_app_name(),
                crate::lang::translate("Service is running".to_owned())
            ))*/
            .with_tooltip(tooltip(0))
            .with_icon(icon)
            .with_icon_as_template(true) // mac only
            .build()?,
    );
    let _tray_icon = Arc::new(Mutex::new(_tray_icon));

    let menu_channel = MenuEvent::receiver();
    let tray_channel = TrayEvent::receiver();
    #[cfg(windows)]
    let (ipc_sender, ipc_receiver) = std::sync::mpsc::channel::<Data>();
    let mut docker_hiden = false;

    let open_func = move || {
        if cfg!(not(feature = "flutter")) {
	        crate::run_me(Vec::<&str>::new()).ok();
	        std::process::exit(0);
	        //crate::run_me::<&str>(vec![]).ok();
	        //return;
        }
        #[cfg(target_os = "macos")]
        crate::platform::macos::handle_application_should_open_untitled_file();
        //#[cfg(target_os = "windows")]
        //{
			//crate::run_me(Vec::<&str>::new()).ok();
			//std::process::exit(0);
			/*
			use std::os::windows::process::CommandExt;
            use std::process::Command;
            Command::new("cmd")
                .arg("/c")
                .arg("start hoptodesk://")
                .creation_flags(winapi::um::winbase::CREATE_NO_WINDOW)
                .spawn()
                .ok();
				*/
        //}
        #[cfg(target_os = "linux")]
        if !std::process::Command::new("xdg-open")
            .arg("hoptodesk://")
            .spawn()
            .is_ok()
        {
            crate::run_me::<&str>(vec![]).ok();
        }
    };

    #[cfg(windows)]
    std::thread::spawn(move || {
        start_query_session_count(ipc_sender.clone());
    });
    #[cfg(windows)]
    let mut last_click = std::time::Instant::now();
    event_loop.run(move |_event, _, control_flow| {
        if !docker_hiden {
            #[cfg(target_os = "macos")]
            crate::platform::macos::hide_dock();
            docker_hiden = true;
        }
        *control_flow = ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(100),
        );

        if let Ok(event) = menu_channel.try_recv() {
            if event.id == quit_i.id() {
                if !crate::check_process("--server", false) {
                    *control_flow = ControlFlow::Exit;
                    return;
                }
                if !crate::platform::uninstall_service(false) {
                    *control_flow = ControlFlow::Exit;
                }
            } else if event.id == open_i.id() {
				open_func();
            /*} else if event.id == stopserv_i.id() {
                #[cfg(target_os = "windows")]
                {
                	Config::set_option("stop-service".into(), "Y".into());
					let cmds = format!(
						"
					chcp 65001
					sc stop {app_name}
					",
						app_name = crate::get_app_name(),
					);
					if let Err(_err) = crate::platform::windows::run_cmds(cmds, false, "uninstall") {
						Config::set_option("stop-service".into(), "".into());
					}
				}*/
            }
        }

        if let Ok(_event) = tray_channel.try_recv() {
            #[cfg(target_os = "windows")]
            if _event.click_type == tray_icon::ClickType::Left
                || _event.click_type == tray_icon::ClickType::Double
            {
                if last_click.elapsed() < std::time::Duration::from_secs(1) {
                    return;
                }
                open_func();
                last_click = std::time::Instant::now();
            }
        }

        #[cfg(windows)]
        if let Ok(data) = ipc_receiver.try_recv() {
            match data {
                Data::ControlledSessionCount(count) => {
                    _tray_icon
                        .lock()
                        .unwrap()
                        .as_mut()
                        .map(|t| t.set_tooltip(Some(tooltip(count))));
                }
                _ => {}
            }
        }
    });
}

#[cfg(windows)]
#[tokio::main(flavor = "current_thread")]
async fn start_query_session_count(sender: std::sync::mpsc::Sender<Data>) {
    let mut last_count = 0;
    loop {
        if let Ok(mut c) = crate::ipc::connect(1000, "").await {
            let mut timer = tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    res = c.next() => {
                        match res {
                            Err(err) => {
                                log::error!("ipc connection closed: {}", err);
                                break;
                            }

                            Ok(Some(Data::ControlledSessionCount(count))) => {
                                if count != last_count {
                                    last_count = count;
                                    sender.send(Data::ControlledSessionCount(count)).ok();
                                }
                            }
                            _ => {}
                        }
                    }

                    _ = timer.tick() => {
                        c.send(&Data::ControlledSessionCount(0)).await.ok();
                    }
                }
            }
        }
        hbb_common::sleep(1.).await;
    }
}