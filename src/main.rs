mod ui;
mod auth;
mod guest;
mod db;
mod weather;
mod logger;
mod menu;

use rusqlite::Connection;
use anyhow::Result;
use std::io::{self, Write};

fn main() -> Result<()> {
    // Initialize main database
    let mut conn = Connection::open("users.db")
        .expect("Failed to open or create database file.");
    db::init_user_db(&conn).expect("Failed to initialize database schema.");

    // Initialize logger
    let mut logger_conn = logger::init_logger_db().expect("Failed to initialize logger database.");

    // Show front page UI
    ui::front_page_ui();

    // Main loop
    loop {
        match prompt_input() {
            Some(choice) => match choice.trim() {
                // === [1] USER LOGIN ===
                "1" => {
                    ui::user_login_ui();
                    match auth::login_user(&conn, &logger_conn) {
                        Ok(Some((username, role))) => {
                            // call menu.rs version of main_menu
                            menu::main_menu(&mut conn, &logger_conn, &username, &role)?;
                        }
                        Ok(None) => {
                            println!("Invalid credentials or login canceled.");
                            ui::front_page_ui();
                        }
                        Err(e) => {
                            eprintln!("Login error: {e}");
                            ui::front_page_ui();
                        }
                    }
                }

                // === [2] GUEST LOGIN ===
                "2" => {
                    ui::user_login_ui();
                    match guest::guest_login_user(&mut conn, &logger_conn) {
                        Ok(Some(username)) => {
                            menu::main_menu(&mut conn, &logger_conn, &username, "guest")?;
                        }
                        Ok(None) => {
                            println!("Guest login failed or canceled.");
                            ui::front_page_ui();
                        }
                        Err(e) => {
                            eprintln!("Guest login error: {e}");
                            ui::front_page_ui();
                        }
                    }
                }

                // === [3] ABOUT ===
                "3" => {
                    ui::about_ui();
                    wait_for_enter();
                    ui::front_page_ui();
                }

                // === [4] EXIT ===
                "4" => {
                    println!("Goodbye!");
                    break;
                }

                _ => println!("Invalid choice. Please enter 1–4.\n"),
            },
            None => {
                println!("End of input detected. Exiting...");
                break;
            }
        }
    }

    Ok(())
}

// ===============================================================
// Prompt user for input
// ===============================================================
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

// ===============================================================
// Pause until user presses ENTER
// ===============================================================
fn wait_for_enter() {
    print!("Press ENTER to continue...");
    let _ = io::stdout().flush();
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
}
