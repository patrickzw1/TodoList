use tauri::Manager;

#[tauri::command]
pub async fn open_sticky_window<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    minimize_main: bool,
) -> Result<(), String> {
    let sticky = match app.get_webview_window("sticky-note") {
        Some(sticky) => sticky,
        None => tauri::WebviewWindowBuilder::new(
            &app,
            "sticky-note",
            tauri::WebviewUrl::App("index.html?mode=sticky".into()),
        )
        .title("TodoList 桌面便签")
        .inner_size(360.0, 330.0)
        .min_inner_size(320.0, 220.0)
        .resizable(true)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .build()
        .map_err(|error| error.to_string())?,
    };
    if sticky.is_minimized().map_err(|error| error.to_string())? {
        sticky.unminimize().map_err(|error| error.to_string())?;
    }
    sticky.show().map_err(|error| error.to_string())?;
    if minimize_main {
        sticky.set_focus().map_err(|error| error.to_string())?;
        if let Some(main) = app.get_webview_window("main") {
            main.minimize().map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn open_main_window<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    let main = match app.get_webview_window("main") {
        Some(main) => main,
        None => {
            let config = app
                .config()
                .app
                .windows
                .iter()
                .find(|window| window.label == "main")
                .ok_or("TodoList main window configuration is missing")?;
            // Creating a WebView2 window must stay in an async command on Windows.
            tauri::WebviewWindowBuilder::from_config(&app, config)
                .and_then(|builder| builder.build())
                .map_err(|error| error.to_string())?
        }
    };
    if main.is_minimized().map_err(|error| error.to_string())? {
        main.unminimize().map_err(|error| error.to_string())?;
    }
    main.show().map_err(|error| error.to_string())?;
    main.set_focus().map_err(|error| error.to_string())?;
    Ok(())
}
