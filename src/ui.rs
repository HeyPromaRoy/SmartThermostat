use colored::*;


pub fn front_page_ui() {
    // colors definition
    let roof_color = Color::Magenta;
    let wall_color = Color::BrightGreen;
    let label_color = Color::BrightYellow;
    let divider_color = Color::BrightBlue;
    let thermo_outline = Color::White;
    let thermo_fill = Color::Red;
    let menu_color = Color::BrightYellow;

    // Center offset — adjust to move left/right if needed
    let pad = "                   "; //spaces for centering

    // Roof
    println!("{}{}", pad, "     _____________".color(roof_color));
    println!("{}{}", pad, "   _/             \\_".color(roof_color));
    println!("{}{}", pad, " _/                 \\_".color(roof_color));
    println!("{}{}", pad, "/_____________________\\".color(roof_color));
    
    println!(
        "{}{}{}{}",
        pad,
        "|".color(wall_color),
        " BIG HOME THERMOSTAT ".color(label_color).bold(),
        "|".color(wall_color)
    );
    println!(
        "{}{}{}{}",
        pad,
        "|".color(wall_color),
        "  -----------------  ".color(divider_color),
        "|".color(wall_color)
    );

    // Thermometer body (white + red fill)
    let lines = vec![
        "       _______       ",
        "      /       \\      ",
        "      |   | _ |      ",
        "      | _ |   |      ",
        "      |   | _ |      ",
        "      | _ |   |      ",
        "      |   | _ |      ",
        "      |  (_)  |      ",
        "      \\_______/      ",
    ];

    for l in lines {
        let redified = l.replace("|", &"|".color(thermo_fill).to_string());
        
        println!(
            "{}{}{}{}",
            pad,
            "|".color(wall_color),
            redified.color(thermo_outline),
            "|".color(wall_color)
        );
    }
    // Base
    println!("{}{}", pad, "|_____________________|".color(wall_color));

    // Menu section
    println!();
    println!(
        "{}{}",
        "    ",
        "[1] User Login   [2] Guest Login   [3] About Application   [4] Exit"
            .color(menu_color)
            .bold()
    );
    println!(); // add an extra blank line for readability
    print!("{}", "Select an option [1-4]: ".bold().color(Color::Cyan));
}

pub fn user_login_ui() {
    let port_spc = " ".repeat(11);
    let port_bar = "=".repeat(46);
    let spacing = " ".repeat(10);
    println!("{}{}", port_spc, port_bar.color(Color::BrightGreen));
    println!("{}{}{}", port_spc, spacing ,"BIG HOME LOGIN PORTAL".color(Color::BrightYellow));
    println!("{}{}", port_spc, port_bar.color(Color::BrightGreen));
}

pub fn homeowner_ui() {
    let bar_color = Color::Magenta;
    let menu_bar = "=".repeat(46);
    let menu_spc = " ".repeat(11);
    let spacing1 = " ".repeat(7);
    let spacing2 = " ".repeat(8);

    println!("{}{}", spacing1, menu_bar.color(bar_color));
    println!("{}{}{}", spacing1, menu_spc, "HOMEOWNER MAIN MENU".bold().color(Color::BrightYellow));
    println!("{}{}", spacing1, menu_bar.color(bar_color));

    println!("{}{}", spacing2, "[1] View profile              |  [6] View System Status".color(Color::White));
    println!("{}{}", spacing2, "[2] Manage Guests             |  [7] Profile Settings".color(Color::White));
    println!("{}{}", spacing2, "[3] Indoor Sensing            |  [8] Energy Usage".color(Color::White));
    println!("{}{}", spacing2, "[4] Outdoor Weather           |  [9] Energy Comparison".color(Color::White));
    println!("{}{}", spacing2, "[5] HVAC Control              |  ".color(Color::White));
    println!("{}{}", spacing2, "[A] Request a Technician      |  [B] View Active Grants".color(Color::White));
    println!("{}{}", spacing2, "              [0] Log out".color(Color::Red));
    
     println!(); 
    print!("{}", "Select an option [0-9, A-B]: ".bold().color(Color::Cyan));

}

pub fn admin_ui() {
let bar_color = Color::Red;
    let menu_bar = "=".repeat(48);
    let menu_spc = " ".repeat(14);
    let spacing1 = " ".repeat(7);
    let spacing2 = " ".repeat(8);

    println!("{}{}", spacing1, menu_bar.color(bar_color));
    println!("{}{}{}", spacing1, menu_spc, "ADMIN MAIN MENU".bold().color(Color::BrightYellow));
    println!("{}{}", spacing1, menu_bar.color(bar_color));

    println!("{}{}", spacing2, "[1] Show my profile           |  [5] View security logs".color(Color::White));
    println!("{}{}", spacing2, "[2] Register a user           |  [6] Clear user lockouts".color(Color::White));
    println!("{}{}", spacing2, "[3] View user(s)              |".color(Color::White));
    println!("{}{}", spacing2, "[4] Manage Users              |".color(Color::White));
    println!("{}{}", spacing2, "              [0] Log out".color(Color::Red));

    println!(); // add an extra blank line for readability
    print!("{}", "Select an option [0-6]: ".bold().color(Color::Cyan));
}

pub fn technician_ui(){
    let bar_color = Color::Red;
    let menu_bar = "=".repeat(46);
    let spacing1 = " ".repeat(7);
    let menu_spc = " ".repeat(11);
    let spacing2 = " ".repeat(8);

    println!("{}{}", spacing1, menu_bar.color(bar_color));
    println!("{}{}{}", spacing1, menu_spc, "TECHNICIAN MAIN MENU".bold().color(Color::BrightYellow));
    println!("{}{}", spacing1, menu_bar.color(bar_color));

    println!("{}{}", spacing2, "[1] Show my profile   |  [6] View System Status".color(Color::White));
    println!("{}{}", spacing2, "[2] View all jobs     |  [7] Indoor sensing".color(Color::White));
    println!("{}{}", spacing2, "[3] Access job        |  [8] Outdoor weather".color(Color::White));
    println!("{}{}", spacing2, "[4] Manage guest(s)   |  [9] Profile settings".color(Color::White));
    println!("{}{}", spacing2, "[5] Run diagnostics   |".color(Color::White));
    println!("{}{}", spacing2, "              [0] Log out".color(Color::Red));
    
    println!(); // add an extra blank line for readability
    print!("{}", "Select an option [0-9]: ".bold().color(Color::Cyan));
}

pub fn guest_ui() {
    let bar_color = Color::BrightBlue;
    let menu_bar = "=".repeat(46);
    let menu_spc = " ".repeat(14);
    let spacing1 = " ".repeat(7);
    let spacing2 = " ".repeat(20);

    println!("{}{}", spacing1, menu_bar.color(bar_color));
    println!("{}{}{}", spacing1, menu_spc, "GUEST MAIN MENU".bold().color(Color::BrightYellow));
    println!("{}{}", spacing1, menu_bar.color(bar_color));

    println!("{}{}", spacing2, "[1] View Profile".color(Color::White));
    println!("{}{}", spacing2, "[2] Indoor Sensor".color(Color::White));
    println!("{}{}", spacing2, "[3] Outdoor Weather".color(Color::White));
    println!("{}{}", spacing2, "[4] HVAC Control".color(Color::White));
    println!("{}{}", spacing2, "[5] Choose Profile".color(Color::White));
    println!("{}{}", spacing2, "[0] Log out".color(Color::Red));

    println!(); // add an extra blank line for readability
    print!("{}", "Select an option [0-5]: ".bold().color(Color::Cyan));
    
}

pub fn manage_guest_menu() {
    
    use std::io::Write;
    
    let bar_color = Color::Magenta;
    let title_color = Color::BrightYellow;
    let text_color = Color::White;

    let menu_bar = "=".repeat(46);
    let menu_spc = " ".repeat(14);
    let spacing1 = " ".repeat(7);
    let spacing2 = " ".repeat(8);

    println!("{}{}", spacing1, menu_bar.color(bar_color));
    println!("{}{}{}", spacing1,  menu_spc, "GUEST MANAGEMENT MENU".bold().color(title_color));
    println!("{}{}", spacing1, menu_bar.color(bar_color));
    
    println!("{}{}", spacing2, "[1] Register Guest".color(text_color));
    println!("{}{}", spacing2, "[2] View guest(s)".color(text_color));
    println!("{}{}", spacing2, "[3] Reset Guest Pin".color(text_color));
    println!("{}{}", spacing2, "[4] Enable/Disable Guest Account".color(text_color));
    println!("{}{}", spacing2, "[5] Delete Guest Account".color(text_color));
    println!("{}{}", spacing2, "[6] Return to User Menu".color(text_color));

    println!();
    print!("{}","Select an option [1-4]: ".bold().color(Color::Cyan));
    std::io::stdout().flush().expect("Failed to flush stdout");

}

pub fn about_ui() {
    let border_color = Color::BrightBlue;
    let title_color = Color::BrightYellow;
    let label_color = Color::BrightCyan;
    let text_color = Color::White;

    let bar = "=".repeat(56);
    let pad = " ".repeat(7);

    println!("\n{}{}", pad, bar.color(border_color));
    println!("{}{}", " ".repeat(22), "BIG HOME THERMOSTAT SYSTEM".bold().color(title_color));
    println!("{}{}", pad, bar.color(border_color));
    println!("{}{} {}", pad,
        "Developed by:".color(label_color).bold(),
        "Team ThermoRust".color(text_color));
    println!("{}{} {}", pad,
        "   Author(s):".color(label_color).bold(),
        "Tahsinur Rahman, Hsiao-Yin Peng,".color(text_color));
    println!("{}{}{}", pad, " ".repeat(14), "Proma Roy, Md Ariful Islam Fahim".color(text_color));
    println!("{}{} {}", pad, "     Version:".color(label_color).bold(), "1.0.0".color(text_color));
    println!("{}{}", pad, " Description:".color(label_color).bold());
    println!("{}{}", pad, "   A smart home control system that manages users,".color(text_color));
    println!("{}{}", pad, "   guests, and integrates real-time weather updates".color(text_color));
    println!("{}{}", pad, "   using data from the NOAA API.".color(text_color));

    println!("{}{}", pad, bar.color(border_color));
    println!();
}

pub fn hvac_control_ui(user_role: &str) {
    let bar_color = Color::Cyan;
    let title_color = Color::BrightYellow;
    let text_color = Color::White;

    let menu_bar = "=".repeat(46);
    let menu_spc = " ".repeat(16);
    let spacing1 = " ".repeat(7);
    let spacing2 = " ".repeat(8);

    println!("{}{}", spacing1, menu_bar.color(bar_color));
    println!("{}{}{}", spacing1, menu_spc, "HVAC CONTROL PANEL".bold().color(title_color));
    println!("{}{}", spacing1, menu_bar.color(bar_color));
    
    println!("{}{}", spacing2, "[1] Change Mode (Heat/Cool/Auto/Fan/Off)".color(text_color));
    println!("{}{}", spacing2, "[2] View Current Status".color(text_color));
    
    // Different menu options based on user role
    if user_role == "homeowner" {
        // Homeowners: Choose Profile option
        println!("{}{}", spacing2, "[3] Choose Profile".color(text_color));
        println!("{}{}", spacing2, "[4] Return to Main Menu".color(text_color));
        println!();
        print!("{}","Select an option [1-4]: ".bold().color(Color::Cyan));
    } else if user_role == "guest" {
        // Guests: No option 3 (already have Choose Profile in main menu)
        println!("{}{}", spacing2, "[3] Return to Main Menu".color(text_color));
        println!();
        print!("{}","Select an option [1-3]: ".bold().color(Color::Cyan));
    } else {
        // Technicians only: Include diagnostics
        println!("{}{}", spacing2, "[3] Run Diagnostics".color(text_color));
        println!("{}{}", spacing2, "[4] Return to Main Menu".color(text_color));
        println!();
        print!("{}","Select an option [1-4]: ".bold().color(Color::Cyan));
    }
}

#[allow(dead_code)]
pub fn hvac_status_ui(temp: f32, target: f32, mode: &str, status: &str) {
    let bar_color = Color::Cyan;
    let title_color = Color::BrightYellow;
    let text_color = Color::White;
    let value_color = Color::BrightGreen;

    let menu_bar = "=".repeat(46);
    let menu_spc = " ".repeat(17);
    let spacing1 = " ".repeat(7);
    let spacing2 = " ".repeat(8);

    println!("{}{}", spacing1, menu_bar.color(bar_color));
    println!("{}{}{}", spacing1, menu_spc, "HVAC STATUS".bold().color(title_color));
    println!("{}{}", spacing1, menu_bar.color(bar_color));
    
    println!("{}{} {}", spacing2, "Current Temperature:".color(text_color), format!("{:.1}°C", temp).color(value_color));
    println!("{}{} {}", spacing2, "Target Temperature:".color(text_color), format!("{:.1}°C", target).color(value_color));
    println!("{}{} {}", spacing2, "Operation Mode:".color(text_color), mode.color(value_color));
    println!("{}{} {}", spacing2, "Current Status:".color(text_color), status.color(value_color));

    println!();
    println!("{}{}", spacing2, "Press Enter to continue...".color(Color::Cyan));
}

pub fn profile_selection_ui(profiles: &[crate::db::ProfileRow]) {
    let bar_color = Color::BrightCyan;
    let title_color = Color::BrightYellow;
    let text_color = Color::White;

    let menu_bar = "=".repeat(46);
    let menu_spc = " ".repeat(15);
    let spacing1 = " ".repeat(7);
    let spacing2 = " ".repeat(8);

    println!("{}{}", spacing1, menu_bar.color(bar_color));
    println!("{}{}{}", spacing1, menu_spc, "CHOOSE HVAC PROFILE".bold().color(title_color));
    println!("{}{}", spacing1, menu_bar.color(bar_color));
    
    // Display all profiles dynamically
    for (idx, profile) in profiles.iter().enumerate() {
        let description = profile.description.as_deref().unwrap_or("Custom profile");
        println!("{}{}", spacing2, format!("[{}] {} - {}", idx + 1, profile.name, description).color(text_color));
    }
    
    println!("{}{}", spacing2, "[0] Cancel".color(Color::Red));

    println!();
    print!("{}", format!("Select a profile [0-{}]: ", profiles.len()).bold().color(Color::Cyan));
}





