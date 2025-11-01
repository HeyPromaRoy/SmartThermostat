use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use rpassword::read_password;
use std::io::{self, Write};
use zeroize::Zeroizing;

use crate::auth;
use crate::db;
use crate::function::wait_for_enter;

pub fn homeowner_request_tech(conn: &mut Connection) -> Result<()> {

    // Get current user from in-process session
    let actor = {
        let guard = auth::ACTIVE_SESSION
            .lock()
            .map_err(|_| anyhow::anyhow!("SESSION lock poisoned"))?;
        guard.clone().ok_or_else(|| anyhow::anyhow!("No user is currently logged in"))?
    };

    // Verify role is homeowner
    let role: Option<String> = conn
        .query_row(
            "SELECT user_status FROM users WHERE username = ?1 COLLATE NOCASE",
            params![&actor],
            |r| r.get(0),
        )
        .optional()?;
    let Some(role) = role else {
        println!("User record not found.");
        return Ok(());
    };
    if role != "homeowner" {
        println!("Only homeowners can request a technician (your role: '{}').", role);
        return Ok(());
    }

        let _ = db::sweep_expire_grants(conn);

 // BLOCK if homeowner already has an active job
    let has_active = {
        let mut found_any = false;
        {
            let mut active_stmt = conn.prepare(
                r#"
                SELECT job_id, technician_username, status, access_minutes, grant_expires
                  FROM technician_jobs
                 WHERE homeowner_username = ?1 COLLATE NOCASE
                   AND status IN ('ACCESS_GRANTED', 'TECH_ACCESS')
                   AND grant_expires > datetime('now')
                 ORDER BY grant_expires DESC
                "#,
            )?;
            let mut rows = active_stmt.query(params![&actor])?;

            while let Some(r) = rows.next()? {
                if !found_any {
                    println!("\nYou already have an active technician access grant:");
                    println!("{:<7} {:<15} {:<12} {:<6} {:<19}",
                             "job_id", "technician", "status", "mins", "expires");
                    found_any = true;
                }
                let jid: i64 = r.get(0)?;
                let tech: String = r.get(1)?;
                let status: String = r.get(2)?;
                let mins: i64 = r.get(3)?;
                let expires: String = r.get(4)?;
                println!("{:<7} {:<15} {:<12} {:<6} {:<19}", jid, tech, status, mins, expires);
            }
    
        }
        found_any
    };
    if has_active {
        return Ok(());
    }
    
    
    // ---- Read request description with retry; accept single-line if already valid
    const MIN_LEN: usize = 20;  //if your DB CHECK is 20–200
    const MAX_LEN: usize = 200;

    let desc: String = loop {
        println!("\nHow can we help you?");
        println!("(Describe the issue. A single line is fine; blank line ends multi-line.)");
        println!("These tasks typically require 30-120 minutes.");
        println!("Guidance:\n  • Quick checks: ~30m\n  • Standard diagnostics & fixes: ~60m\n  • Multi-device / complex: ~90–120m\n");
        print!("Type your request:\n> ");
        io::stdout().flush().ok();

        let mut acc = String::new();
        let mut saw_content = false;

        loop {
            let mut line = String::new();
            let n = io::stdin().read_line(&mut line)?;
            if n == 0 { break; } // EOF
            let trimmed = line.trim_end_matches(&['\r','\n'][..]);

            // ignore leading empty line
            if trimmed.is_empty() && !saw_content {
                print!("> ");
                io::stdout().flush().ok();
                continue;
            }
            // blank line ends multi-line
            if trimmed.is_empty() { break; }

            if !acc.is_empty() { acc.push(' '); }
            acc.push_str(trimmed);
            saw_content = true;

            // Accept immediately if already within bounds
            let len = acc.chars().count();
            if (MIN_LEN..=MAX_LEN).contains(&len) {
                break;
            }
            // otherwise prompt continuation
            print!("… ");
            io::stdout().flush().ok();
        }

        // sanitize and validate
        let mut d = acc.trim().to_string();
        d.retain(|c| !c.is_control());
        d = d.split_whitespace().map(str::to_string).collect::<Vec<_>>().join(" ");
        let len = d.chars().count();
        if len < MIN_LEN || len > MAX_LEN {
            println!("Description must be {}–{} characters (current: {}). Try again.", MIN_LEN, MAX_LEN, len);
            continue;
        }
        break d;
    };

    // Minutes prompt/validation
    let minutes: i64 = loop {
        print!("Please specify minutes for the technician to access [30|60|90|120]: ");
        io::stdout().flush().ok();
        let mut s = String::new();
        io::stdin().read_line(&mut s)?;
        let s = s.trim();
        match s.parse::<i64>() {
            Ok(m) if [30, 60, 90, 120].contains(&m) => break m,
            _ => println!("Enter one of: 30, 60, 90, 120."),
        }
    };

    // Technician list
    let techs: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT username FROM users
             WHERE user_status = 'technician' AND is_active = 1
             ORDER BY username COLLATE NOCASE ASC",
        )?;
        let iter = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut v = Vec::new();
        for t in iter { v.push(t?); }
        v
    };
    if techs.is_empty() {
        println!("No active technicians available.");
        return Ok(());
    }
    println!("\nAvailable technicians:");
    for (i, t) in techs.iter().enumerate() {
        println!("  {}) {}", i + 1, t);
    }
    let idx: usize = loop {
        print!("\nSelect a technician by number: ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        match input.trim().parse::<usize>() {
            Ok(n) if n >= 1 && n <= techs.len() => break n - 1,
            _ => println!("Invalid selection. Enter 1..{}", techs.len()),
        }
    };
    let technician_username = &techs[idx];

    // Create job/grant (ASSIGNED)
    let job_id = db::grant_technician_access(conn, &actor, technician_username, minutes, &desc)?;
    println!("\nRequest recorded:");
    println!("  Homeowner: {}", actor);
    println!("  Technician: {}", technician_username);
    println!("  Minutes: {}", minutes);
    println!("  Job ID: {}", job_id);
    println!("  Status: ASSIGNED");

    Ok(())
}


// ======================================================
//                 TECHNICIAN SIDE
// ======================================================
pub fn tech_list_my_jobs(conn: &Connection) -> Result<()> {
    // session -> username
    let me = {
        let guard = auth::ACTIVE_SESSION
            .lock()
            .map_err(|_| anyhow::anyhow!("SESSION lock poisoned"))?;
        match guard.clone() {
            Some(u) => u,
            None => { println!("No user is currently logged in."); return Ok(()); }
        }
    };

    // role lookup
    let role: Option<String> = conn
        .query_row(
            "SELECT user_status FROM users WHERE username = ?1 COLLATE NOCASE",
            params![&me],
            |r| r.get(0),
        )
        .optional()?;

    let Some(role) = role else {
        println!("User record not found.");
        return Ok(());
    };
    if role != "technician" {
        println!("Only technicians can view technician jobs.");
        return Ok(());
    }

    // query jobs
    let mut stmt = conn.prepare(
        r#"
        SELECT job_id, homeowner_username, status, access_minutes, grant_start, grant_expires, updated_at
        FROM technician_jobs WHERE technician_username = ?1 COLLATE NOCASE
         ORDER BY grant_expires DESC
        "#,
    )?;
    let mut rows = stmt.query(params![&me])?;

    println!("\nJobs for technician '{}':", me);
    println!(
        "{:<7} {:<16} {:<12} {:<6} {:<19} {:<19} {:<19}",
        "job_id","homeowner","status","mins","grant_start","grant_expires", "updated_at"
    );

    let mut any = false;
    while let Some(r) = rows.next()? {
        any = true;
        let jid: i64    = r.get(0)?;
        let homeowner: String = r.get(1)?;
        let status: String    = r.get(2)?;
        let mins: i64   = r.get(3)?;
        let gs: String  = r.get(4)?;
        let ge: String  = r.get(5)?;
        let ua: String  = r.get(6)?;
        println!("{:<7} {:<16} {:<12} {:<6} {:<19} {:<19} {:<19}", jid, homeowner, status, mins, gs, ge, ua);
    }
    if !any { println!("(no jobs)"); }
    Ok(())
}


// Technician: start an ASSIGNED job (within grant window)
pub fn tech_access_job(conn: &mut Connection) -> Result<()> {
    
let tech_username = {
        let guard = auth::ACTIVE_SESSION.lock().map_err(|_| anyhow::anyhow!("SESSION lock poisoned"))?;
        match guard.clone() {
            Some(u) => u,
            None => {
                println!("No user is currently logged in.");
                return Ok(());
            }
        }
    };

    // Must be an active technician
    let (role, active): (String, i64) = conn
        .query_row(
            "SELECT user_status, is_active FROM users WHERE username = ?1 COLLATE NOCASE",
            params![&tech_username],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .context("Failed to lookup account")?;
    if role != "technician" || active != 1 {
        println!(
            "Access denied: '{}' is not an active technician.",
            tech_username
        );
        wait_for_enter();
        return Ok(());
    }

    let _ = db::sweep_expire_grants(conn);


    // Load ASSIGNED jobs within TTL into an owned Vec
let jobs: Vec<(i64, String, String, i64, String, String, String)> = {
    let mut stmt = conn.prepare(
        r#"
        SELECT
            job_id, homeowner_username, status,
            CAST((strftime('%s', grant_expires) - strftime('%s','now'))/60 AS INTEGER) AS ttl_minutes,
            updated_at, job_desc, grant_expires
        FROM technician_jobs WHERE technician_username = ?1 COLLATE NOCASE AND status IN ('ACCESS_GRANTED','TECH_ACCESS')
          AND grant_expires > datetime('now')
        ORDER BY grant_expires ASC
        "#,
    )?;
    let mut rows = stmt.query(params![&tech_username])?;

    let mut v = Vec::new();
    while let Some(r) = rows.next()? {
        v.push((
            r.get::<_, i64>(0)?,     // job_id
            r.get::<_, String>(1)?,  // homeowner_username
            r.get::<_, String>(2)?,  // status
            r.get::<_, i64>(3)?,     // ttl_minutes
            r.get::<_, String>(4)?,  // updated_at
            r.get::<_, String>(5)?,  // job_desc
            r.get::<_, String>(6)?,  // grant_expires
        ));
    }
    v
};


    if jobs.is_empty() {
        println!("No active granted jobs.");
        wait_for_enter();
        return Ok(());
    }

    // Show table
    println!("\n=== Use an Access Grant ===");
    println!(
        "{:<4} {:<18} {:<12} {:<8} {:<20} {}",
        "No.", "Homeowner", "Status", "TTL(m)", "Updated", "Description"
    );
    for (i, j) in jobs.iter().enumerate() {
        // tuple fields: j.0..j.5
        println!(
            "{:<4} {:<18} {:<12} {:<8} {:<20} {}",
            i + 1,
            j.1, // homeowner
            j.2, // status
            j.3, // ttl_minutes
            j.4, // updated_at
            j.5  // job_desc
        );
    }

    // Select by number
    print!("\nEnter number to start (or blank to abort): ");
    io::stdout().flush().ok();
    let mut sel = String::new();
    io::stdin().read_line(&mut sel).ok();
    let s = sel.trim();
    if s.is_empty() {
        println!("Aborted.");
        wait_for_enter();
        return Ok(());
    }
    let idx = s.parse::<usize>().unwrap_or(0);
    let Some(job) = jobs.get(idx.saturating_sub(1)) else {
        println!("Invalid selection.");
        wait_for_enter();
        return Ok(());
    };

    // Destructure to named locals
    let (job_id, homeowner_username, _status, _ttl_minutes, updated_at, job_desc, grant_expires) = job.clone();

    // Show the full description before auth/use
    println!("\nJob #{} for homeowner '{}'", job_id, homeowner_username);
    println!("Valid until  : {}", grant_expires);
    println!("  Status    : {}", _status);          // or a fixed label if you prefer
    println!("  Updated   : '{}'", updated_at);      // this was the line with the error
    println!("  Description: {}", job_desc);

    // Step-up re-auth
    println!("\nSecurity check for technician '{}':", tech_username);
    print!("Enter your technician password: ");
    io::stdout().flush().ok();
    let pw_in = Zeroizing::new(read_password()?);
    let pw_trimmed = pw_in.trim_end_matches(['\r', '\n']);

    let stored_hash_opt: Option<String> = conn
        .query_row(
            "SELECT hashed_password FROM users WHERE username = ?1 COLLATE NOCASE AND user_status = 'technician'",
            params![&tech_username],
            |r| r.get(0),
        )
        .optional()?;
    let auth_ok = stored_hash_opt
        .as_deref()
        .map_or(false, |h| auth::verify_password(pw_trimmed, h).unwrap_or(false));
    if !auth_ok {
        println!("Authentication failed.");
        wait_for_enter();
        return Ok(());
    }

    // Delegate the state change to DB: either claim TECH_ACCESS or flip to EXPIRED
    match db::access_job(conn, job_id, &tech_username)? {
        Some((_home, desc, expires)) => {
            println!("Access is valid for job #{} until {}.", job_id, expires);
            println!("Description: {}", desc);
            wait_for_enter();
            Ok(())
        }
        None => {
            println!("Grant expired or not available; ask homeowner to re-grant.");
            wait_for_enter();
            Ok(())
        }
    }
}