use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Manager;
use todolist_desktop_lib::window_actions::{open_main_window, open_sticky_window};

#[test]
fn restores_main_from_config_without_replacing_sticky_or_duplicating_main() {
    let mut context = mock_context(noop_assets());
    let config: tauri::Config = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    context.config_mut().app.windows = config.app.windows;
    // Simulate a closed main window while its reusable configuration remains.
    context.config_mut().app.windows[0].create = false;
    let app = mock_builder().build(context).unwrap();
    let sticky = tauri::WebviewWindowBuilder::new(&app, "sticky-note", Default::default())
        .build()
        .unwrap();
    assert!(app.get_webview_window("main").is_none());

    tauri::async_runtime::block_on(open_main_window(app.handle().clone())).unwrap();
    assert!(app.get_webview_window("main").is_some());
    assert!(app.get_webview_window(sticky.label()).is_some());
    assert_eq!(app.webview_windows().len(), 2);

    tauri::async_runtime::block_on(open_main_window(app.handle().clone())).unwrap();
    assert_eq!(app.webview_windows().len(), 2);
}

#[test]
fn reuses_an_existing_main_without_needing_creation_configuration() {
    let app = mock_builder().build(mock_context(noop_assets())).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    tauri::async_runtime::block_on(open_main_window(app.handle().clone())).unwrap();
    assert!(app.get_webview_window(main.label()).is_some());
    assert_eq!(app.webview_windows().len(), 1);
}

#[test]
fn reports_missing_main_configuration_instead_of_silently_succeeding() {
    let app = mock_builder().build(mock_context(noop_assets())).unwrap();
    let result = tauri::async_runtime::block_on(open_main_window(app.handle().clone()));
    assert!(result.unwrap_err().contains("main window configuration"));
    assert!(app.webview_windows().is_empty());
}

#[test]
fn creates_one_sticky_window_and_keeps_the_main_window_alive() {
    let app = mock_builder().build(mock_context(noop_assets())).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    tauri::async_runtime::block_on(open_sticky_window(app.handle().clone(), false)).unwrap();
    assert!(app.get_webview_window("sticky-note").is_some());
    assert!(app.get_webview_window(main.label()).is_some());

    tauri::async_runtime::block_on(open_sticky_window(app.handle().clone(), true)).unwrap();
    assert_eq!(app.webview_windows().len(), 2);
    assert!(app.get_webview_window(main.label()).is_some());
}

#[test]
fn dragging_permission_is_scoped_to_the_sticky_window() {
    let capability: serde_json::Value =
        serde_json::from_str(include_str!("../capabilities/sticky.json")).unwrap();
    assert_eq!(capability["windows"], serde_json::json!(["sticky-note"]));
    assert_eq!(
        capability["permissions"],
        serde_json::json!(["core:window:allow-start-dragging"])
    );
}

#[test]
fn backup_file_dialogs_are_scoped_to_the_main_window() {
    let capability: serde_json::Value =
        serde_json::from_str(include_str!("../capabilities/backup-dialog.json")).unwrap();
    assert_eq!(capability["windows"], serde_json::json!(["main"]));
    assert_eq!(
        capability["permissions"],
        serde_json::json!(["dialog:allow-open", "dialog:allow-save"])
    );
}
