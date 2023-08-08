#[rustfmt::skip]
fn print_usage(program_name: &String) {
    println!("\n{} [OPTIONS]", program_name);
    println!("Cloud syncing utility");
    println!("\t sync  <folder> <account_name> [--fresh|-f]         : syncs the folder to cloud provider, --fresh flag does a fetch from begining");
    println!("\t login <gdrive|onedrive>                            : prints the login url");
    println!("\t save  <gdrive|onedrive> <account_name> <auth_code> : Requests access token and saves it to config file");
    println!("\t help                                               : prints this menu ");
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let program_name = &args[0];

    if args.len() < 2 {
        eprintln!("ERROR: Missing options");
        print_usage(program_name);
        std::process::exit(-1);
    }

    let command = &args[1];

    let res = match command.as_str() {
        "sync" => cloudsync::sync(&args),
        "login" => cloudsync::login(&args),
        "save" => cloudsync::save(&args),
        _ => {
            print_usage(program_name);
            Err("Invalid arguments".to_string())
        }
    };

    if let Err(err) = res {
        eprintln!("ERROR: {err}");
        std::process::exit(-1);
    }
}
