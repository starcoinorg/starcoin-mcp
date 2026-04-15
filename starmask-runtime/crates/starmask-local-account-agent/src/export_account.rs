#![forbid(unsafe_code)]

use std::{
    convert::TryFrom,
    fs::OpenOptions,
    io::{self, Read, Write},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde_json::json;
use starcoin_account::{AccountManager, account_storage::AccountStorage};
use starcoin_account_api::AccountPrivateKey;
use starcoin_config::RocksdbConfig;
use starcoin_crypto::ValidCryptoMaterialStringExt;
use starcoin_types::{account_address::AccountAddress, genesis_config::ChainId};

#[derive(Debug, Parser)]
#[command(name = "local-account-export")]
#[command(about = "Export one Starcoin local account private key from an account vault")]
struct Cli {
    #[arg(long)]
    wallet_dir: PathBuf,
    #[arg(long)]
    address: AccountAddress,
    #[arg(long, default_value_t = 254)]
    chain_id: u8,
    #[arg(long)]
    output_file: PathBuf,
    #[arg(long)]
    password_stdin: bool,
    #[arg(long)]
    force: bool,
    #[arg(long)]
    json: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let password = read_password(cli.password_stdin)?;

    let storage = AccountStorage::create_from_path(&cli.wallet_dir, RocksdbConfig::default())
        .with_context(|| format!("failed to open account vault {}", cli.wallet_dir.display()))?;
    let manager = AccountManager::new(storage, ChainId::new(cli.chain_id))
        .context("failed to open account manager")?;
    let private_key_bytes = manager
        .export_account(cli.address, password.trim_end_matches(['\n', '\r']))
        .with_context(|| format!("failed to export account {}", cli.address))?;
    if private_key_bytes.is_empty() {
        bail!(
            "account {} has no exportable private key; it may be read-only",
            cli.address
        );
    }

    let private_key = AccountPrivateKey::try_from(private_key_bytes.as_slice())
        .context("exported account private key is invalid")?;
    let encoded = private_key
        .to_encoded_string()
        .context("failed to encode account private key")?;
    write_private_key_file(&cli.output_file, &encoded, cli.force)?;

    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "address": cli.address.to_string(),
                "wallet_dir": cli.wallet_dir,
                "output_file": cli.output_file,
                "generated_at_unix_seconds": SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|duration| duration.as_secs())
                    .unwrap_or_default(),
            }))?
        );
    } else {
        println!(
            "account private-key export created: {}",
            cli.output_file.display()
        );
        println!("address:                {}", cli.address);
        println!("wallet_dir:             {}", cli.wallet_dir.display());
    }

    Ok(())
}

fn read_password(password_stdin: bool) -> Result<String> {
    if password_stdin {
        let mut password = String::new();
        io::stdin()
            .read_to_string(&mut password)
            .context("failed to read account password from stdin")?;
        if password.trim_end_matches(['\n', '\r']).is_empty() {
            bail!("account password cannot be empty");
        }
        return Ok(password);
    }

    let password = rpassword::prompt_password("Account password: ")
        .context("failed to read account password")?;
    if password.is_empty() {
        bail!("account password cannot be empty");
    }
    Ok(password)
}

fn write_private_key_file(path: &PathBuf, encoded: &str, force: bool) -> Result<()> {
    if path.exists() && !force {
        bail!(
            "output file already exists: {}; pass --force to overwrite it",
            path.display()
        );
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }

    let mut options = OpenOptions::new();
    options.write(true).truncate(true);
    if force {
        options.create(true);
    } else {
        options.create_new(true);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("failed to open output file {}", path.display()))?;
    file.write_all(encoded.as_bytes())
        .context("failed to write account private-key export")?;
    file.write_all(b"\n")
        .context("failed to finish account private-key export")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to restrict permissions on {}", path.display()))?;
    }

    Ok(())
}
