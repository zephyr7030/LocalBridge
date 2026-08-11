pub mod app;
pub mod commands;
pub mod credentials;
pub mod diagnostics;
pub mod mcp;
pub mod privilege;
pub mod runtime;
pub mod settings;
pub mod state;
pub mod tray;
pub mod tunnel;
pub mod workspace;

pub const PRODUCT_NAME: &str = "LocalBridge";

pub fn build_app() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
}

pub fn run() {
    build_app()
        .run(tauri::generate_context!())
        .expect("LocalBridge 启动失败");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_name_is_stable() {
        assert_eq!(PRODUCT_NAME, "LocalBridge");
    }
}
