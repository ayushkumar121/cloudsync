use std::{
    collections::HashMap,
    io::Read,
    os::unix::prelude::FileExt,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
pub mod onedrive;

const BOLD_START: &str = "\x1b[1m";
const BOLD_END: &str = "\x1b[0m";

#[derive(Serialize, Deserialize, Clone)]
pub enum SyncService {
    GDrive,
    Onedrive,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Token {
    pub access_token: String,
    pub refresh_token: String,
    pub valid_till: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Account {
    pub service: SyncService,
    pub token: Token,

    // Storing last sync information
    pub last_synced: u64,
    pub attributes: HashMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct Config {
    accounts: HashMap<String, Account>,
}

// Assuming args
// clousync sync <folder> <account_name>
pub fn sync(args: &Vec<String>) -> Result<(), String> {
    if args.len() != 4 {
        return Err("Incorrect no of arguments".to_string());
    }

    let folder = &args[2];
    let account_name = &args[3];

    let folder_path =
        std::fs::canonicalize(folder).map_err(|err| format!("Invalid path: {}", err))?;

    // TODO: check if folder is valid folder

    let folder_path_str = folder_path.to_string_lossy().to_string();

    let config_path = config_path();
    let config_data = std::fs::read_to_string(config_path)
        .map_err(|err| format!("Cannot read config: {}", err))?;

    let mut config: Config = serde_json::from_str(config_data.as_str())
        .map_err(|err| format!("Cannot read config: {}", err))?;

    if let Some(account) = config.accounts.get_mut(account_name) {
        sync_files(account_name, account, &folder_path_str)?;
    } else {
        return Err("Unknown account name please login first".to_string());
    }

    Ok(())
}

// Assuming args
// clousync login <gdrive|onedrive>
pub fn login(args: &Vec<String>) -> Result<(), String> {
    if args.len() != 3 {
        return Err("Incorrect no of arguments".to_string());
    }

    match args[2].as_str() {
        "onedrive" => {
            let login_url = onedrive::get_oauth_url();
            println!(
                "{}Copy paste this url to browser{}: \n\n{}",
                BOLD_START, BOLD_END, login_url
            );
        }
        "gdrive" => todo!(),
        _ => {
            return Err("Please specify a service".to_string());
        }
    };

    Ok(())
}

// Assuming args
// clousync save <gdrive|onedrive> <account_name> <auth_code>
pub fn save(args: &Vec<String>) -> Result<(), String> {
    if args.len() != 5 {
        return Err("Incorrect no of arguments".to_string());
    }

    let service = match args[2].as_str() {
        "gdrive" => SyncService::GDrive,
        "onedrive" => SyncService::Onedrive,
        _ => {
            return Err("Incorrect sync service".to_string());
        }
    };

    let account_name = &args[3];
    let auth_code = &args[4];
    let token = match service {
        SyncService::GDrive => todo!(),
        SyncService::Onedrive => onedrive::get_token(auth_code, "authorization_code"),
    }?;

    let account = Account {
        service: SyncService::Onedrive,
        token,
        last_synced: 0,
        attributes: HashMap::new(),
    };

    save_account(account_name, &account)?;
    println!("INFO: Account saved");

    Ok(())
}

fn config_path() -> String {
    // TODO: figure out home dir for windows
    let home = std::env!("HOME");
    format!("{home}/.config/cloudsync.json")
}

fn save_account(account_name: &str, account: &Account) -> Result<(), String> {
    let config_path = config_path();
    let mut config_file = std::fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .open(config_path)
        .map_err(|err| format!("Cannot create config file: {}", err))?;

    let mut config_data = String::new();
    config_file
        .read_to_string(&mut config_data)
        .map_err(|err| format!("Cannot read config file: {}", err))?;

    let mut config = match serde_json::from_str::<Config>(config_data.as_str()) {
        Ok(config) => config,
        Err(_) => Config {
            accounts: HashMap::new(),
        },
    };

    config
        .accounts
        .insert(account_name.to_owned(), account.clone());
    config_data = serde_json::to_string(&config).unwrap();

    config_file
        .write_all_at(config_data.as_bytes(), 0)
        .map_err(|err| format!("Cannot write config to file: {}", err))?;

    Ok(())
}

fn refresh_token(account: &mut Account) -> Result<(), String> {
    let token = match account.service {
        SyncService::GDrive => todo!(),
        SyncService::Onedrive => {
            onedrive::get_token(account.token.refresh_token.as_str(), "refresh_token")
        }
    }?;

    account.token = token;
    Ok(())
}

#[derive(Debug)]
pub struct CloudFile {
    pub folder: String,
    pub file_name: String,
}

fn download_file(parent: &String, file: &CloudFile) -> Result<(), String> {
    let full_folder_path = format!("{}{}", parent, file.folder);
    std::fs::create_dir_all(&full_folder_path)
        .map_err(|err| format!("Cannot create folder: {}", err))?;

    let full_file_path = format!("{}/{}", full_folder_path, file.file_name);

    // TODO: Download actual file
    std::fs::File::options()
        .write(true)
        .create(true)
        .open(full_file_path)
        .map_err(|err| format!("Cannot create file: {}", err))?;

    Ok(())
}

fn sync_files(
    account_name: &str,
    account: &mut Account,
    folder_path: &String,
) -> Result<(), String> {
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();

    if since_the_epoch > account.token.valid_till {
        refresh_token(account)?;
    }

    let files = match account.service {
        SyncService::GDrive => todo!(),
        SyncService::Onedrive => onedrive::get_drive_delta(account)?,
    };

    // TODO: Download all files
    for file in files {
        if let Err(err) = download_file(folder_path, &file) {
            println!(
                "\nERROR: Cannot sync file : {}/{} :\n{}",
                file.folder, file.file_name, err
            );
        }
    }

    // TODO: Upload local files

    save_account(account_name, account)?;

    Ok(())
}
