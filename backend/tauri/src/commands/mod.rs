use wbook_core::types::Port;

///
/// This command is used to get the port number from the backend.
///
#[tauri::command]
pub fn get_port(port: tauri::State<Port>) -> u16 {
    port.0
}
