mod auth; mod db; mod function; mod guest; mod hvac; mod logger;
mod menu; mod ui;  mod profile; mod senser; mod technician; mod weather; mod energy;
mod diagnostic;

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn main() -> Result<()> {
    let integrity_check = false;
    if integrity_check {
        // 1) Check the hash list file is exist
        let manifest = "INTEGRITY.sha256";
        if !Path::new(manifest).exists() {
            bail!("Can't find {manifest}, please generate the hash list first(scripts/gen_integrity.ps1)");
        }

        // 2) Verify each line: format of each line "<hash><two spaces><filename>"
        let content = fs::read_to_string(manifest).context("Read INTEGRITY.sha256 fail")?;
        let mut ok = 0usize;
        let mut bad = 0usize;

        for (idx, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() { continue; }
            let (expected_hash, file) = match line.split_once("  ") {
                Some(t) => t,
                None => bail!("INTEGRITY.sha256 line {} error format: {}", idx + 1, line),
            };

            let data = fs::read(file).with_context(|| format!("Read fail: {file}"))?;
            let got = hex::encode(Sha256::digest(&data));
            if got == expected_hash {
                // println!("✅ OK  {file}"); // for debug: check integrity
                ok += 1;
            } else {
                // eprintln!("❌ MISMATCH  {file}"); // for debug: check integrity
                bad += 1;
            }
        }

        println!("---\nPASS: {ok}, FAIL: {bad}");
        if bad > 0 {
            bail!("Fail to check the integrity of the source code, stop excuting");
        }
        
        }

    // 3) After passing the check, run the system
    run_app()

}

fn run_app() -> Result<()> {
    // Initialize unified system database (users + logs + lockouts)
    let mut conn = db::get_connection().expect("Failed to initialize system database.");

    let _anon_token = db::update_session(&conn, None)?;
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
                            let _token = db::update_session(&conn, Some(&username))?;
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
                    let _ = db::end_session(&conn, "");
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
