mod ui;
mod function;
mod auth;
mod db;
mod weather;
mod logger;
mod menu;
mod senser;

use rusqlite::Connection;
use anyhow::Result;
use std::io::{self, Write};

fn main() -> Result<()> {
    // Initialize main database
    let mut conn = Connection::open("users.db")
        .expect("Failed to open or create database file.");
    db::init_db(&conn).expect("Failed to initialize database schema.");

    // Initialize logger
    let logger_conn = logger::init_logger_db().expect("Failed to initialize logger database.");

    // Show front page UI
    ui::front_page_ui();

    // Main loop
    loop {
        match function::prompt_input() {
            Some(choice) => match choice.trim() {
                // === [1] USER LOGIN ===
                "1" => {
                    ui::user_login_ui();
                    match auth::login_user(&conn, &logger_conn) {
                        Ok(Some((username, role))) => {
                            // call menu.rs version of main_menu
                            menu::main_menu(&mut conn, &username, &role)?;
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
                    match auth::guest_login_user(&conn, &logger_conn) {
                        Ok(Some(username)) => {
                            menu::main_menu(&mut conn, &username, "guest")?;
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
                    function::wait_for_enter();
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

