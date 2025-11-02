// Temporary password reset utility
// Run with: rustc reset_password.rs && reset_password.exe

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2, Algorithm, Version, Params,
};

fn main() {
    println!("=== Password Hash Generator ===");
    println!("Enter the new password you want to set:");
    
    let mut password = String::new();
    std::io::stdin().read_line(&mut password).unwrap();
    let password = password.trim();
    
    // Use same parameters as your app
    let params = Params::new(65_536, 3, 1, None).expect("Invalid Argon2 params");
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let salt = SaltString::generate(&mut OsRng);
    
    match argon2.hash_password(password.as_bytes(), &salt) {
        Ok(hash) => {
            println!("\nGenerated hash:");
            println!("{}", hash);
            println!("\nTo update the admin password, run:");
            println!("sqlite3 system.db \"UPDATE users SET hashed_password = '{}' WHERE username = 'Admin' COLLATE NOCASE;\"", hash);
        }
        Err(e) => {
            eprintln!("Error hashing password: {}", e);
        }
    }
}
