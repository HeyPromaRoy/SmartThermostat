mod ui; mod auth; mod guest; mod db; mod logger; mod admin;
mod menu; mod weather; mod senser; mod function;

use anyhow::Result;
use std::io::{self, Write};

fn main() -> Result<()> {
    // Initialize unified system database (users + logs + lockouts)
    let mut conn = db::get_connection().expect("Failed to initialize system database.");

    // Show front page UI
    ui::front_page_ui();

    // Main program loop
    loop {
        match function::prompt_input() {
            Some(choice) => match choice.trim() {
                // === [1] USER LOGIN ===
                "1" => {
                    ui::user_login_ui();
                    match auth::login_user(&conn) {
                        Ok(Some((username, role))) => {
                            // Proceed to role-based menu
                            menu::main_menu(&mut conn, &username, &role)?;
                        }
                        Ok(None) => {
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
                    match guest::guest_login_user(&mut conn) {
                        Ok(Some(username)) => {
                            let username: String = username;
                            menu::main_menu(&mut conn, &username, "guest")?;
                        }
                        Ok(None) => {
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
