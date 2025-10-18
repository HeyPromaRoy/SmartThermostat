use std::io::{self, Write};

/// ==============================================
/// Helper: Prompt user for input
/// ==============================================
pub fn prompt_input() -> Option<String> {
    if io::stdout().flush().is_err() {
        eprintln!("Error flushing stdout.");
        return None;
    }

    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(0) => None, // EOF
        Ok(_) => Some(input.trim().to_string()),
        Err(e) => {
            eprintln!("Error reading input: {e}");
            None
        }
    }
}


/// ==============================================
/// Helper: Pause until user presses ENTER
/// ==============================================
pub fn wait_for_enter() {
    print!("Press ENTER to continue...");
    let _ = io::stdout().flush();
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
}

pub fn flush_buffer() {
    let _ = io::stdout().flush();
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
}
