# Smart Thermostat — Secure CLI HVAC Prototype

This repository contains "Big Home Thermostat" — a secure, role-based, command-line smart thermostat prototype implemented in Rust as a course project for the Secure Systems Engineering course (City College of New York). It demonstrates secure authentication, session management, tamper-evident integrity checks, role-based access control, HVAC simulation, energy tracking, and safe SQLite database usage.

This README documents what the project does, how it is designed, how to build/run it, and the security decisions made. 

*Project Goals*
- *Secure baseline*: demonstrate common, practical defenses (Argon2id password hashing, lockouts, session tokens, DB PRAGMAs).
- *Role-based control*: homeowners, guests, technicians, and admins with different privileges.
- *Reproducible CLI demo*: interactive CLI UI with HVAC simulation, profiles, sensor mocks, energy reports, and technician workflows.

*What it does*
- Provides a CLI thermostat system that lets users log in (or use a guest account), apply HVAC profiles, change temperature and mode, view indoor/outdoor data, view and store energy usage, and run diagnostics.
- Enforces security controls: strong password rules, Argon2id hashed credentials, session tokens, account lockouts and session lockouts, and security/audit logging.
- Maintains a unified SQLite database (system.db) for users, sessions, logs, HVAC state, profiles, and energy usage. The database is hardened with secure PRAGMA settings.

*When/why it was built*
This project was implemented as a group assignment to practice applying secure design patterns to an embedded/IoT-like control system: authentication, safe storage, integrity checks, least-privilege access, and auditability. It is suitable as a portfolio example showing security-aware engineering applied to a realistic system.

*Repository layout*
 - Cargo.toml — Rust manifest and dependencies.
 - src/ — main program and modules:
	 - main.rs — entry point; performs an integrity check (INTEGRITY.sha256), initializes DB and runs the CLI loop.
	 - lib.rs — re-exports modules.
	 - auth.rs — registration, login/logout, Argon2id hashing, password policies, in-memory session guard.
	 - db.rs — full SQLite schema, migrations, profiles, session management, and helper DB functions.
	 - logger.rs — security logging, lockout logic, session failure handling, and file-backed audit log.
	 - hvac.rs — HVAC simulation, modes, temperature range limits, and diagnostics.
	 - ui.rs — terminal UI renders for menus and control panels (colored ASCII art).
	 - energy.rs — energy tracking, mock data generation, aggregation and storage routines.
	 - weather.rs — (NOAA fetch wrapper) outdoor weather integration used by UI and reports.
	 - senser.rs — sensor interface (mocked for demo/testing) for indoor values.
	 - technician.rs, guest.rs, profile.rs, menu.rs, function.rs, diagnostic.rs — supporting flows and helpers.
 - tests/ — integration tests covering DB init, auth, hvac behavior, sensors, and other subsystems.

*Security and design highlights*
- Password hashing: uses Argon2id (argon2 crate) with memory-hard parameters (≈64 MiB, 3 iterations) to resist offline cracking.
- Password policy: non-guest accounts must meet character classes (upper/lower/digit/special) and minimum length; guest PINs enforced numeric + length.
- Secrets in memory: zeroize / Zeroizing wrappers are used to reduce secret lifetime in memory.
- Session tokens: tokens are generated with OS RNG, returned to the process, but stored as a hashed digest (BLAKE3) in the DB to avoid storing plaintext session tokens.
- Lockouts & anti-enumeration: progressive account lockouts, session lockouts, fake verification delays, and constant-time verification behavior for unknown users to reduce timing and enumeration attacks.
- DB hardening: PRAGMA journal_mode=WAL, synchronous=FULL, foreign_keys=ON, secure_delete=ON, temp_store=MEMORY to improve durability and reduce sensitive leftovers.
- Auditability: security_log table and an appended security.log file record important events (logins, lockouts, profile changes, HVAC actions).
- Integrity check: main.rs reads INTEGRITY.sha256 to ensure the repo files match expected SHA-256 hashes before running (prevents tampering during demonstration). The project references scripts/gen_integrity.ps1 in code comments; you can regenerate a matching file locally (see Run instructions).

*How it works (high-level runtime flow)*
1. main loads and verifies INTEGRITY.sha256 (the manifest is expected to be in the repository root).
2. The unified SQLite database (system.db) is initialized/migrated via db::init_system_db.
3. The CLI front page is displayed (ui::front_page_ui). Users choose to log in, guest login, read About, or Exit.
4. Authentication (auth) handles login and session creation. Successful login stores a session entry in session_state and sets an in-memory session guard to prevent concurrent logins from the same CLI instance.
5. Role-based menus (menu.rs, ui.rs) provide different capabilities for homeowner, guest, technician, and admin.
6. HVAC subsystem (hvac.rs) reads a mocked sensor value (senser.rs), decides actions (heating/cooling/fan), logs actions, and persists state to the DB.
7. Energy tracking (energy.rs) can generate mock data, aggregate usages, and persist energy records for historical reporting.

*Build and run (local dev)*
Prerequisites:
- Rust toolchain (stable) and cargo installed.

Build:
bash
# from repository root
cargo build --release


Run (development):
bash
# Run the program (creates `system.db` on first run)
cargo run --release


Integrity manifest (optional but required by main when integrity_check = true):
- The program expects INTEGRITY.sha256 in repository root. To (re)generate a manifest locally that matches the format expected by main ("<sha256><two spaces><filename>"), you can run:
bash
# POSIX/macOS: include desired files and avoid the `target/` directory
find . -type f -not -path './target/*' -not -path './.git/*' -not -name 'system.db' -print0 \
	| xargs -0 shasum -a 256 \
	> INTEGRITY.sha256

Note: main.rs expects two spaces between hash and filename (the shasum output uses two spaces by default). If you change the repo contents, update INTEGRITY.sha256 before running.

Files created at runtime:
- system.db — unified SQLite database (users, sessions, logs, profiles, hvac_state, etc.).
- security.log — appended audit file produced by logger::log_event.

*Run tests*
bash
cargo test

The repository includes integration tests in tests/integration_test.rs that exercise DB init, auth, HVAC, sensors, energy, and other modules.

*Further work / TODOs*
- Add automated CI that runs cargo test and validates INTEGRITY.sha256.
- Provide a small script to regenerate the integrity manifest in a reproducible manner.
- Add unit tests that mock sensors for deterministic HVAC behavior (to complement the integration tests).

*Authors & Credits*
- Team ThermoRust — Proma Roy, Tahsinur Rahman, Hsiao-Yin Peng, Md Ariful Islam Fahim

---

Clone the project

> git clone https://github.com/HeyPromaRoy/SmartThermostat.git

- Set environment variables by creating a .env file (or copying one provided by the project team)  
- The .env file should contain the database key.”

SQLCIPHER_KEY='DATABASE_PASSWORD'

Run the project

> cd SmartThermostat
> cargo run


## Requirements
1. *Database key :* .env
2. *Hash list :* INTEGRITY.sha256 (for integrity check to prevent backdoor injection)