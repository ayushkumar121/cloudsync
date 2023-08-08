# CloudSync
========

CloudSync is CLI tool to sync a given folder
to various cloud providers written in rust

We use last modified metadata associated with 
file to determine which version to keep

## Usage

```shell
$ cloudsync help

cloudsync [OPTIONS]
Cloud syncing utility

	sync  <folder> <account_name> [--fresh|-f]
                 syncs the folder to cloud provider, --fresh flag does a fetch from begining

	login <gdrive|onedrive>
                 prints the login url

	save  <gdrive|onedrive> <account_name> <auth_code>
                 Requests access token and saves it to config file

	help
                 prints this menu 

```

## Features

- Multiple Accounts
- Multiple Cloud Providers

## Supported Cloud Providers

- One Drive
- Google Drive

## References