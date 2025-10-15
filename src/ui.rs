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
        "[1] User Login   [2] Guest Login?   [3] [Placeholder]   [4] Exit"
            .color(menu_color)
            .bold()
    );
    println!();
}

pub fn user_login_ui() {
    let port_spc = "           ";
    let port_bar = "=========================================";
    println!("{}{}", port_spc, port_bar.color(Color::BrightGreen));
    println!("{}{}", port_spc, "          BIG HOME LOGIN PORTAL".color(Color::BrightYellow));
    println!("{}{}", port_spc, port_bar.color(Color::BrightGreen));
}

pub fn homeowner_ui() {
    let bar_color = Color::Magenta;
    let menu_bar = "==============================================";
    let menu_spc = "              ";
    let spacing1 = "       ";
    let spacing2 = "        ";

    println!("{}{}", spacing1, menu_bar.color(bar_color));
    println!("{}{}{}", spacing1, menu_spc, "USER MAIN MENU".bold().color(Color::BrightYellow));
    println!("{}{}", spacing1, menu_bar.color(bar_color));

    println!("{}{}", spacing2, "[1] Register a guest".color(Color::White));
    println!("{}{}", spacing2, "[2] View guest(s)".color(Color::White));
    println!("{}{}", spacing2, "[3] Manage Guests".color(Color::White));
    println!("{}{}", spacing2, "[4] Show my profile".color(Color::White));
    println!("{}{}", spacing2, "[5] Log out".color(Color::White));
    

    println!(); // add an extra blank line for readability
    println!("{}", "Select an option [1-4]".bold().color(Color::Cyan));
}


pub fn guest_ui() {
    let bar_color = Color::BrightBlue;
    let menu_bar = "==============================================";
    let spacing1 = "       ";
    let spacing2 = "        ";

    println!("{}{}", spacing1, menu_bar.color(bar_color));
    println!("{}{}", spacing1, "           TECHNICIAN MAIN MENU".bold().color(Color::BrightYellow));
    println!("{}{}", spacing1, menu_bar.color(bar_color));

    println!("{}{}", spacing2, "[1] Register a guest".color(Color::White));
    println!("{}{}", spacing2, "[2] View guest(s)".color(Color::White));
    println!("{}{}", spacing2, "[3] Manage Guests".color(Color::White));
    println!("{}{}", spacing2, "[4] Show my profile".color(Color::White));
    println!("{}{}", spacing2, "[5] Log out".color(Color::White));
    
}

pub fn admin_ui() {
let bar_color = Color::Red;
    let menu_bar = "==============================================";
    let spacing1 = "       ";
    let spacing2 = "        ";

    println!("{}{}", spacing1, menu_bar.color(bar_color));
    println!("{}{}", spacing1, "              ADMIN MAIN MENU".bold().color(Color::BrightYellow));
    println!("{}{}", spacing1, menu_bar.color(bar_color));

    println!("{}{}", spacing2, "[1] Register a user".color(Color::White));
    println!("{}{}", spacing2, "[2] View guest(s)".color(Color::White));
    println!("{}{}", spacing2, "[3] Manage Guests".color(Color::White));
    println!("{}{}", spacing2, "[4] Show my profile".color(Color::White));
    println!("{}{}", spacing2, "[5] Log out".color(Color::White));
}

pub fn technician_ui(){
    let bar_color = Color::Red;
    let menu_bar = "==============================================";
    let spacing1 = "       ";
    let spacing2 = "        ";

    println!("{}{}", spacing1, menu_bar.color(bar_color));
    println!("{}{}", spacing1, "           TECHNICIAN MAIN MENU".bold().color(Color::BrightYellow));
    println!("{}{}", spacing1, menu_bar.color(bar_color));

    println!("{}{}", spacing2, "[1] Register a guest".color(Color::White));
    println!("{}{}", spacing2, "[2] View guest(s)".color(Color::White));
    println!("{}{}", spacing2, "[3] Manage Guests".color(Color::White));
    println!("{}{}", spacing2, "[4] Show my profile".color(Color::White));
    println!("{}{}", spacing2, "[5] Log out".color(Color::White));
    
}