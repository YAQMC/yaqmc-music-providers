use std::{env, path::PathBuf};
use tokio::net::TcpListener;
use yaqmc_music_providers_backend::{default_config_path, AppConfig, Backend};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "serve".to_owned());
    let mut config_path = default_config_path();
    while let Some(argument) = args.next() {
        if argument == "--config" {
            config_path = PathBuf::from(args.next().ok_or("missing value for --config")?);
        } else if argument == "--path" {
            config_path = PathBuf::from(args.next().ok_or("missing value for --path")?);
        } else {
            return Err(format!("unknown argument: {argument}").into());
        }
    }

    match command.as_str() {
        "init" => {
            AppConfig::write_template(&config_path)?;
            println!("wrote {}", config_path.display());
        }
        "serve" => {
            let config = AppConfig::load(&config_path)?;
            let listener =
                TcpListener::bind((config.listen.host.as_str(), config.listen.port)).await?;
            eprintln!(
                "yaqmc-music-providers listening on http://{}:{}",
                config.listen.host, config.listen.port
            );
            Backend::new(config)?.serve_http(listener).await?;
        }
        "stdio" => {
            let config = AppConfig::load(&config_path)?;
            Backend::new(config)?.serve_stdio().await?;
        }
        _ => return Err(format!("unknown command: {command}").into()),
    }
    Ok(())
}
