use std::{
    io::Read,
    time::{SystemTime, UNIX_EPOCH},
};

use curl::easy::{Easy, Form, List};
use serde::{Deserialize, Serialize};

use crate::{parse_iso_date, urlencode, Account, DriveDelta, DriveDeltaType, Token};

const CLIENT_ID: &str = "3dceca68-abd4-46a1-9e72-9dda8a80d9c1";
const REDIRECT_URL: &str = "https://login.microsoftonline.com/common/oauth2/nativeclient";
const SCOPES: &str = "User.Read%20Files.ReadWrite.All%20offline_access";

pub fn get_oauth_url() -> String {
    let auth_url = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";

    format!(
        "{}?client_id={}&response_type=code&redirect_uri={}&scope={}",
        auth_url, CLIENT_ID, REDIRECT_URL, SCOPES,
    )
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug, Clone)]
struct FileProperties {
    mimeType: Option<String>,
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug, Clone)]
struct FolderProperties {
    childCount: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ParentReference {
    path: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Deleted {
    state: String,
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug, Clone)]
struct OneDriveItem {
    id: String,
    name: Option<String>,

    // Deleted files do not have
    // path in parent reference
    parentReference: ParentReference,

    lastModifiedDateTime: Option<String>,
    file: Option<FileProperties>,
    folder: Option<FolderProperties>,
    deleted: Option<Deleted>,
}

#[derive(Serialize, Deserialize, Debug)]
struct OneDriveListItems {
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,

    #[serde(rename = "@odata.deltaLink")]
    delta_link: Option<String>,
    value: Vec<OneDriveItem>,
}
fn get_delta(account: &mut Account, api_url: &str, items: &mut Vec<OneDriveItem>) {
    let mut headers = List::new();
    headers
        .append(format!("Authorization:Bearer {}", account.token.access_token).as_str())
        .unwrap();

    let mut handle = Easy::new();
    let mut response_body = Vec::new();

    handle.url(api_url).unwrap();
    handle.http_headers(headers).unwrap();
    {
        let mut transfer = handle.transfer();
        transfer
            .write_function(|data| {
                response_body.extend_from_slice(data);
                Ok(data.len())
            })
            .unwrap();
        transfer.perform().unwrap();
    }

    let drive_items = serde_json::from_slice::<OneDriveListItems>(&response_body).unwrap();
    items.extend(drive_items.value);

    // Last page conatins deltaLink for next time
    // sync
    if let Some(delta_link) = drive_items.delta_link {
        let delta_link_key = "delta_link".to_string();
        account.attributes.insert(delta_link_key, delta_link);
    }

    if let Some(next_link) = drive_items.next_link {
        get_delta(account, next_link.as_str(), items);
    }
}

pub fn download_file(account: &Account, item_path: &str) -> Result<Vec<u8>, String> {
    let mut headers = List::new();
    headers
        .append(format!("Authorization:Bearer {}", account.token.access_token).as_str())
        .unwrap();

    let item_path_escaped = urlencode(item_path);
    let api_url = format!(
        "https://graph.microsoft.com/v1.0/me/drive/root:/{}:/content",
        item_path_escaped
    );
    let mut handle = Easy::new();
    let mut response_body = Vec::new();

    handle.url(&api_url).unwrap();
    handle.follow_location(true).unwrap();
    handle.http_headers(headers).unwrap();
    handle.fail_on_error(true).unwrap();
    {
        let mut transfer = handle.transfer();
        transfer
            .write_function(|data| {
                response_body.extend_from_slice(data);
                Ok(data.len())
            })
            .unwrap();
        transfer
            .perform()
            .map_err(|err| format!("Cannot perform request: {}", err))?;
    }

    Ok(response_body)
}

pub fn upload_new_file(
    account: &Account,
    item_path: &str,
    mut contents: &[u8],
) -> Result<String, String> {
    let mut headers = List::new();
    headers
        .append(format!("Authorization:Bearer {}", account.token.access_token).as_str())
        .unwrap();
    headers.append("Content-Type: text/plain").unwrap();

    let item_path_escaped = urlencode(item_path);
    let api_url = format!(
        "https://graph.microsoft.com/v1.0/me/drive/root:{}:/content",
        item_path_escaped
    );
    let mut handle = Easy::new();
    let mut response_body = Vec::new();

    handle.url(&api_url).unwrap();
    handle.http_headers(headers).unwrap();
    handle.put(true).unwrap();
    handle.fail_on_error(true).unwrap();
    {
        let mut transfer = handle.transfer();
        transfer
            .read_function(|into| Ok(contents.read(into).unwrap()))
            .unwrap();

        transfer
            .write_function(|data| {
                response_body.extend_from_slice(data);
                Ok(data.len())
            })
            .unwrap();

        transfer
            .perform()
            .map_err(|err| format!("Cannot perform request: {}", err))?;
    }

    let drive_item: OneDriveItem = serde_json::from_slice(&response_body)
        .map_err(|err| format!("Cannot parse response: {}", err))?;

    Ok(drive_item.id)
}

pub fn delete_file(account: &Account, cloud_id: &str) -> Result<(), String> {
    let mut headers = List::new();
    headers
        .append(format!("Authorization:Bearer {}", account.token.access_token).as_str())
        .unwrap();
    headers.append("Content-Type: text/plain").unwrap();

    let api_url = format!(
        "https://graph.microsoft.com/v1.0/me/drive/items/{}",
        cloud_id
    );
    let mut handle = Easy::new();

    handle.url(&api_url).unwrap();
    handle.http_headers(headers).unwrap();
    handle.custom_request("DELETE").unwrap();
    handle.fail_on_error(true).unwrap();

    handle
        .perform()
        .map_err(|err| format!("Cannot perform request: {}", err))
}

pub fn get_drive_delta(account: &mut Account) -> Result<Vec<DriveDelta>, String> {
    let mut files = Vec::new();
    let root_delta_link = "https://graph.microsoft.com/v1.0/me/drive/root/delta".to_string();

    let delta_link_key = "delta_link".to_string();
    let delta_link = match account.attributes.get(&delta_link_key) {
        Some(val) => val.clone(),
        None => root_delta_link.clone(),
    };

    get_delta(account, &delta_link, &mut files);

    let mut cloud_files = Vec::new();
    for file in files {
        // Skipping folders
        if file.folder.is_some() {
            continue;
        }

        if file.name.is_none() {
            continue;
        }

        let file_name = file.name.unwrap();
        if file_name.is_empty() {
            continue;
        }

        let file_path = if let Some(mut parent) = file.parentReference.path {
            let folder = parent.split_off(12);
            format!("{}/{}", folder, file_name)
        } else {
            format!("/{}", file_name)
        };

        let last_modified = parse_iso_date(&file.lastModifiedDateTime.unwrap());

        cloud_files.push(DriveDelta {
            cloud_id: file.id,
            file_path,
            last_modified,
            delta_type: if file.deleted.is_some() {
                DriveDeltaType::Deleted
            } else {
                DriveDeltaType::CreatedOrModifiled
            },
        });
    }

    Ok(cloud_files)
}

#[derive(Serialize, Deserialize)]
struct MicrosoftGraphToken {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

pub fn get_token(code: &str, grant_type: &str) -> Result<Token, String> {
    let mut form = Form::new();
    form.part("client_id")
        .contents(CLIENT_ID.as_bytes())
        .add()
        .unwrap();
    form.part("redirect_uri")
        .contents(REDIRECT_URL.as_bytes())
        .add()
        .unwrap();
    form.part("grant_type")
        .contents(grant_type.as_bytes())
        .add()
        .unwrap();

    match grant_type {
        "authorization_code" => {
            form.part("code").contents(code.as_bytes()).add().unwrap();
        }
        "refresh_token" => {
            form.part("refresh_token")
                .contents(code.as_bytes())
                .add()
                .unwrap();
        }
        _ => return Err("Invalid grant_type".to_string()),
    };

    let api_url = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
    let mut handle = Easy::new();
    let mut response_body = Vec::new();

    handle.url(api_url).unwrap();
    handle.httppost(form).unwrap();
    handle.fail_on_error(true).unwrap();
    {
        let mut transfer = handle.transfer();
        transfer
            .write_function(|data| {
                response_body.extend_from_slice(data);
                Ok(data.len())
            })
            .unwrap();

        transfer
            .perform()
            .map_err(|err| format!("Cannot perform request: {}", err))?;
    }

    let microsoft_token: MicrosoftGraphToken =
        serde_json::from_slice(&response_body).map_err(|err| {
            format!(
                "Cannot parse response please relogin : {} :\n{}",
                grant_type, err
            )
        })?;

    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();

    Ok(Token {
        access_token: microsoft_token.access_token,
        refresh_token: microsoft_token.refresh_token,
        valid_till: since_the_epoch + microsoft_token.expires_in,
    })
}
