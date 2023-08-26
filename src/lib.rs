use std::{
    collections::HashMap,
    io::{Read, Seek},
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

#[derive(Debug)]
pub enum DriveDeltaType {
    Deleted,
    CreatedOrModifiled,
}

#[derive(Debug)]
pub struct DriveDelta {
    pub cloud_id: String,
    pub file_path: String,
    pub last_modified: u64,
    pub delta_type: DriveDeltaType,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Account {
    pub service: SyncService,
    pub token: Token,
    pub last_synced: u64,
    pub attributes: HashMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct Config {
    accounts: HashMap<String, Account>,
}

#[derive(Serialize, Deserialize)]
struct CloudStateEntry {
    cloud_id: String,
    last_modified: u64,
}

#[derive(Serialize, Deserialize)]
struct CloudState {
    entries: HashMap<String, CloudStateEntry>,
}

pub fn urlencode(data: &str) -> String {
    data.replace(' ', "%20")
}

#[derive(Default)]
struct SyncFlags {
    fresh: bool,
}

// Assuming args
// clousync sync <folder> <account_name> [--fresh/-f]
pub fn sync(args: &Vec<String>) -> Result<(), String> {
    if args.len() < 4 {
        return Err("Incorrect no of arguments".to_string());
    }

    let folder = &args[2];
    let account_name = &args[3];

    let folder_path = std::fs::canonicalize(folder)
        .map_err(|err| format!("Cannot sync to {} because: {}", folder, err))?;

    let mut sync_flags = SyncFlags::default();

    // Parsing flags
    // Flags come after the positional arguments
    for flag in args.iter().skip(4) {
        match flag.as_str() {
            "--fresh" | "-f" => sync_flags.fresh = true,
            _ => {
                return Err("Invalid flags".to_string());
            }
        };
    }

    let folder_path_str = folder_path.to_string_lossy().to_string();

    let config_path = config_path();
    let config_data = std::fs::read_to_string(config_path)
        .map_err(|err| format!("Cannot read config: {}", err))?;

    let mut config: Config = serde_json::from_str(config_data.as_str())
        .map_err(|err| format!("Cannot read config: {}", err))?;

    if let Some(account) = config.accounts.get_mut(account_name) {
        sync_files(account, account_name, &folder_path_str, &sync_flags)?;
    } else {
        return Err("Unknown account name please login first".to_string());
    }

    Ok(())
}

// Assuming args
// clousync login <gdrive|onedrive>
pub fn login(args: &Vec<String>) -> Result<(), String> {
    if args.len() < 3 {
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
    if args.len() < 5 {
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

// NOTE: We're cloning the entire account struct
// So this will be a costly operation
fn save_account(account_name: &str, account: &Account) -> Result<(), String> {
    let config_path = config_path();
    let mut config_file = std::fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
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

// Recursively walk through
fn read_dir_rec(folder: &str, files: &mut HashMap<String, u64>) -> std::io::Result<()> {
    let dir_entries = std::fs::read_dir(folder)?;

    for entry in dir_entries.flatten() {
        let metadata = entry.metadata()?;
        let file_path = entry.path().to_str().unwrap().to_string();

        if metadata.is_dir() {
            read_dir_rec(&file_path, files)?;
        } else {
            let last_modified = metadata
                .modified()?
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();

            files.insert(file_path, last_modified);
        }
    }

    Ok(())
}

fn timestamp() -> u64 {
    let start = SystemTime::now();
    start.duration_since(UNIX_EPOCH).unwrap().as_secs()
}

fn sync_files(
    account: &mut Account,
    account_name: &str,
    folder_to_sync: &String,
    sync_flags: &SyncFlags,
) -> Result<(), String> {
    println!("Syncing {} to {}", folder_to_sync, account_name);

    let now = timestamp();
    if now > account.token.valid_till {
        println!("INFO: Token refreshed");
        refresh_token(account)?;
    }

    if sync_flags.fresh {
        account.last_synced = 0;
        account.attributes = HashMap::new();
    }

    // Getting local changes
    let mut local_files = HashMap::new();
    read_dir_rec(folder_to_sync, &mut local_files)
        .map_err(|err| format!("Cannot walk folder to sync: {}", err))?;

    // Deleting local files incase of
    // fresh sync
    if sync_flags.fresh {
        println!("INFO: Cleaning up local files {}", local_files.len());

        for file_path in local_files.keys() {
            std::fs::remove_file(file_path)
                .map_err(|err| format!("Cannot remove file: {}", err))?;
        }

        local_files = HashMap::new();
    }

    // Creating a scope so file is closed
    // Before we update the last sync
    {
        println!("INFO: Reading cloudstate");

        let cloudstate_file_path = format!("{}/.cloudstate", folder_to_sync);
        let mut cloudstate_file = std::fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .open(cloudstate_file_path)
            .map_err(|err| err.to_string())?;

        let mut cloudstate = if !sync_flags.fresh {
            match serde_json::from_reader(&cloudstate_file) {
                Ok(state) => state,
                Err(_) => CloudState {
                    entries: HashMap::new(),
                },
            }
        } else {
            CloudState {
                entries: HashMap::new(),
            }
        };

        // Getting cloud changes
        let deltas = match account.service {
            SyncService::GDrive => todo!(),
            SyncService::Onedrive => onedrive::get_drive_delta(account)?,
        };

        println!("INFO: Cloud Delta {}", deltas.len());
        println!("INFO: Cloud files {}", cloudstate.entries.len());
        println!("INFO: Local files {}", local_files.len());

        for delta in &deltas {
            // Skip the cloud sync cloud we have
            // already have this file from the last sync
            if account.last_synced >= delta.last_modified {
                continue;
            }

            let (folder, _) = delta.file_path.rsplit_once('/').unwrap();
            let file_path = delta.file_path.clone();
            let full_file_path = format!("{}{}", folder_to_sync, file_path);
            let local_modified = local_files.get(&full_file_path).map_or(0, |val| *val);

            // Making sure cloud files get priotity on
            // fresh fetch
            let cloud_modified = if sync_flags.fresh {
                timestamp()
            } else {
                delta.last_modified
            };

            match delta.delta_type {
                DriveDeltaType::Deleted => {
                    if cloud_modified > local_modified {
                        println!("INFO: Deleting local file {}", full_file_path);

                        match std::fs::remove_file(&full_file_path) {
                            Ok(_) => {
                                local_files.remove(&full_file_path);
                            }
                            Err(err) => {
                                println!("ERROR: Cannot remove file: {}", err)
                            }
                        };

                        cloudstate.entries.remove(&file_path);
                    }
                }
                DriveDeltaType::CreatedOrModifiled => {
                    if cloud_modified > local_modified {
                        println!("INFO: Downloading {}", file_path);

                        let full_folder_path = format!("{}/{}", folder_to_sync, folder);
                        std::fs::create_dir_all(&full_folder_path)
                            .map_err(|err| err.to_string())?;

                        let response = match account.service {
                            SyncService::GDrive => todo!(),
                            SyncService::Onedrive => onedrive::download_file(account, &file_path),
                        };

                        match response {
                            Ok(contents) => {
                                std::fs::write(&full_file_path, contents)
                                    .map_err(|err| err.to_string())?;

                                let ts = timestamp();
                                cloudstate.entries.insert(
                                    file_path,
                                    CloudStateEntry {
                                        cloud_id: delta.cloud_id.to_string(),
                                        last_modified: ts,
                                    },
                                );
                                local_files.insert(full_file_path, ts);
                            }
                            Err(err) => {
                                println!("ERROR: Downloading file {}", err);
                            }
                        };
                    } else {
                        cloudstate.entries.remove(&file_path);
                    }
                }
            }
        }

        // Uploading locally modified files
        for (file_path, local_modified) in &local_files {
            let local_modified = *local_modified;
            let drive_relative_path = file_path.split(folder_to_sync).last().unwrap();

            let result = cloudstate.entries.get(drive_relative_path);
            let is_file_modified = result.is_some()
                && local_modified > account.last_synced
                && local_modified > result.unwrap().last_modified;

            if is_file_modified || result.is_none() {
                match std::fs::read(file_path) {
                    Ok(file_contents) => {
                        println!("INFO: Uploading {}", file_path);

                        let response = match account.service {
                            SyncService::GDrive => todo!(),
                            SyncService::Onedrive => onedrive::upload_new_file(
                                account,
                                drive_relative_path,
                                &file_contents,
                            ),
                        };

                        match response {
                            Ok(cloud_id) => {
                                let ts = timestamp();
                                cloudstate.entries.insert(
                                    drive_relative_path.to_string(),
                                    CloudStateEntry {
                                        cloud_id,
                                        last_modified: ts,
                                    },
                                );
                            }
                            Err(err) => {
                                println!("ERROR: Uploading file: {}", err);
                            }
                        };
                    }
                    Err(err) => {
                        println!("ERROR: Reading file {}: {}", file_path, err);
                    }
                }
            }
        }

        // Removing cloud files
        let mut cloudfiles_to_deleted = Vec::new();
        {
            for file_path in cloudstate.entries.keys() {
                let entry = &cloudstate.entries.get(file_path).unwrap();
                let full_file_path = format!("{}{}", folder_to_sync, file_path);

                if local_files.get(&full_file_path).is_none() {
                    println!("INFO: Cloud deleting file {}", file_path);

                    let response = match account.service {
                        SyncService::GDrive => todo!(),
                        SyncService::Onedrive => onedrive::delete_file(account, &entry.cloud_id),
                    };

                    match response {
                        Ok(_) => {}
                        Err(err) => {
                            println!("ERROR: Cloud deleting file: {}", err);
                        }
                    };
                    cloudfiles_to_deleted.push(file_path.clone());
                }
            }
        }

        for file_path in cloudfiles_to_deleted {
            cloudstate.entries.remove(&file_path);
        }

        // Truncating the file
        cloudstate_file
            .set_len(0)
            .map_err(|err| format!("Cannot write to file: {}", err))?;
        cloudstate_file.seek(std::io::SeekFrom::Start(0)).unwrap();

        serde_json::to_writer(cloudstate_file, &cloudstate).map_err(|err| err.to_string())?;
    }

    // Save changes to account
    account.last_synced = timestamp();
    save_account(account_name, account)?;

    Ok(())
}

// Assuming date 2023-08-06T13:23:00.093Z (ISO format)
// @Returns unix timestamp
fn parse_iso_date(date_time_str: &str) -> u64 {
    let (date_str, time_str) = date_time_str.split_once('T').unwrap();
    let date_tokens: Vec<&str> = date_str.split('-').collect();

    let year: u64 = date_tokens[0].parse().unwrap();
    let month: u64 = date_tokens[1].parse().unwrap();
    let date: u64 = date_tokens[2].parse().unwrap();

    let time_tokens: Vec<&str> = time_str.split(':').collect();

    let hours: u64 = time_tokens[0].parse().unwrap();
    let minutes: u64 = time_tokens[1].parse().unwrap();

    let seconds_str = &time_tokens[2][0..2];
    let seconds: u64 = seconds_str.parse().unwrap();

    fn days_per_year(year: u64) -> u64 {
        if year % 4 == 0 && year % 100 != 0 || year % 400 == 0 {
            366
        } else {
            365
        }
    }

    fn days_per_month(month: u64, year: u64) -> u64 {
        match month {
            1 => 31,
            2 => {
                if days_per_year(year) == 365 {
                    28
                } else {
                    29
                }
            }
            3 => 31,
            4 => 30,
            5 => 31,
            6 => 30,
            7 => 31,
            8 => 31,
            9 => 30,
            10 => 31,
            11 => 30,
            12 => 31,
            _ => unreachable!(),
        }
    }

    let mut days_since_epoch = 0;
    for y in 1970..year {
        days_since_epoch += days_per_year(y);
    }

    let mut days_in_year_so_far = 0;
    for m in 1..month {
        days_in_year_so_far += days_per_month(m, year);
    }
    days_since_epoch += days_in_year_so_far + (date - 1);

    let seconds_in_hour = 60 * 60;

    (days_since_epoch * 24 * seconds_in_hour) + hours * seconds_in_hour + minutes * 60 + seconds
}

#[cfg(test)]
mod tests {
    use crate::parse_iso_date;

    #[test]
    fn test_date_parsing() {
        assert_eq!(parse_iso_date("2023-08-06T13:23:00Z"), 1691328180);
    }
}
