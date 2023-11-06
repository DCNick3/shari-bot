use crate::dispatcher::DownloadDispatcher;
use crate::downloader::tiktok::TikTokDownloader;
use crate::downloader::youtube::YoutubeDownloader;
use anyhow::Context;
use anyhow::{bail, Result};
use futures::{StreamExt, TryStreamExt};
use grammers_client::{Client, Config, InitParams, SignInError};
use grammers_session::Session;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

mod bot;
mod config;
mod dispatcher;
mod downloader;
mod init_tracing;

async fn connect_and_login(config: &config::Telegram) -> Result<Client> {
    let mut catch_up = false;

    let session = match &config.session_storage {
        Some(session_storage) => {
            let session_storage = Path::new(session_storage);
            if session_storage.exists() {
                info!("Loading saved session from {}", session_storage.display());
                // only request catch up when loading our own session, not a prepared or a new one
                catch_up = true;
                Some(Session::load_file(session_storage).context("Loading session")?)
            } else {
                info!("No session file found, creating a new session");
                None
            }
        }
        None => {
            warn!("No session storage configured, creating a new session. This will create dangling sessions on restarts!");
            None
        }
    };

    let session = match session {
        Some(session) => session,
        None => match &config.account {
            config::TelegramAccount::PreparedSession { session } => {
                info!("Loading session from config");
                Session::load(session).context("Loading session")?
            }
            _ => Session::new(),
        },
    };

    let client = Client::connect(Config {
        session,
        api_id: config.api_id,
        api_hash: config.api_hash.clone(),
        params: InitParams {
            catch_up,
            ..Default::default()
        },
    })
    .await
    .context("Connecting to telegram")?;

    if !client
        .is_authorized()
        .await
        .context("failed to check whether we are signed in")?
    {
        info!("Not signed in, signing in...");

        match &config.account {
            config::TelegramAccount::PreparedSession { .. } => {
                bail!("Prepared session is not signed in, please sign in manually and provide the session file")
            }
            config::TelegramAccount::Bot { token } => {
                info!("Signing in as bot");
                client
                    .bot_sign_in(token)
                    .await
                    .context("Signing in as bot")?;
            }
            config::TelegramAccount::User { phone } => {
                info!("Signing in as user");
                let login_token = client
                    .request_login_code(phone)
                    .await
                    .context("Requesting login code")?;

                info!("Asked telegram for login code, waiting for it to be entered");

                let mut logic_code = String::new();
                std::io::stdin()
                    .read_line(&mut logic_code)
                    .context("Reading login code")?;
                let logic_code = logic_code.strip_suffix('\n').unwrap();

                match client.sign_in(&login_token, &logic_code).await {
                    Ok(_) => {}
                    Err(SignInError::PasswordRequired(password_token)) => {
                        info!(
                            "2FA Password required, asking for it. Password hint: {}",
                            password_token.hint().unwrap()
                        );
                        let mut password = String::new();
                        std::io::stdin()
                            .read_line(&mut password)
                            .context("Reading password")?;
                        let password = password.strip_suffix('\n').unwrap();

                        client
                            .check_password(password_token, password)
                            .await
                            .context("Checking password")?;
                    }
                    Err(e) => {
                        return Err(e).context("Signing in as user");
                    }
                }
            }
        }

        if config.session_storage.is_some() {
            info!("Signed in, saving session");
            save_session(&client, config)?;
        } else {
            warn!("Signed in, but no session storage configured. This will leave dangling sessions on restarts!");
        }
    }

    Ok(client)
}

fn save_session(client: &Client, config: &config::Telegram) -> Result<()> {
    if let Some(session_storage) = &config.session_storage {
        debug!("Saving session to {}", session_storage);
        std::fs::write(session_storage, client.session().save()).context("Saving session")?;
    }

    Ok(())
}

async fn save_session_periodic(client: &Client, config: &config::Telegram) -> Result<()> {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60 * 5));

    loop {
        interval.tick().await;
        save_session(client, config)?;
    }
}

// #[allow(unused)]
// mod remuxer;

// #[tracing::instrument]
// async fn remux_example() -> anyhow::Result<()> {
//     let client = reqwest::ClientBuilder::new()
//         .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/92.0.4515.115 Safari/537.36")
//         .build()?;
//
//     let video_url = "https://rr1---sn-xguxaxjvh-8v1e.googlevideo.com/videoplayback?expire=1656138395&ei=O1a2YumEDJqn1gKWi5noCw&ip=5.3.162.204&id=o-AKV_SXG8stglh-ay6GcLwt2g8_n9Z_-0P2ig_3ePOUYG&itag=136&source=youtube&requiressl=yes&mh=z8&mm=31%2C29&mn=sn-xguxaxjvh-8v1e%2Csn-n8v7znsy&ms=au%2Crdu&mv=m&mvi=1&pl=22&initcwndbps=1536250&vprv=1&mime=video%2Fmp4&gir=yes&clen=18685498&dur=90.399&lmt=1579533885693773&mt=1656116466&fvip=2&keepalive=yes&fexp=24001373%2C24007246&c=ANDROID&txp=1316222&sparams=expire%2Cei%2Cip%2Cid%2Citag%2Csource%2Crequiressl%2Cvprv%2Cmime%2Cgir%2Cclen%2Cdur%2Clmt&sig=AOq0QJ8wRQIgJsWH0s9_cu8urBc_MplZ5UfpfWPmYthsixfGhEDW4i8CIQC76GXuws_QC4ENSzifnZIxgeZkPnAV1R5RDiaY1NMW4Q%3D%3D&lsparams=mh%2Cmm%2Cmn%2Cms%2Cmv%2Cmvi%2Cpl%2Cinitcwndbps&lsig=AG3C_xAwRQIgHBITziGocRprHOU4WOdDuFntewoCYUjU507HBeAuM_kCIQDlj5wFwNyXv3hUPaHcWbp5Dy1hVaaRF9MrCoYk6he5Sg%3D%3D";
//     let audio_url = "https://rr1---sn-xguxaxjvh-8v1e.googlevideo.com/videoplayback?expire=1656138415&ei=T1a2YrSLDryix_APura1QA&ip=5.3.162.204&id=o-AIF1QbedyHpYsWPdxc5OQ6KzOaDtTx8XGe4lT5HJAx3z&itag=140&source=youtube&requiressl=yes&mh=z8&mm=31%2C29&mn=sn-xguxaxjvh-8v1e%2Csn-n8v7znsy&ms=au%2Crdu&mv=m&mvi=1&pl=22&initcwndbps=1536250&vprv=1&mime=audio%2Fmp4&gir=yes&clen=1464847&dur=90.464&lmt=1579533878395083&mt=1656116466&fvip=2&keepalive=yes&fexp=24001373%2C24007246&c=ANDROID&txp=1311222&sparams=expire%2Cei%2Cip%2Cid%2Citag%2Csource%2Crequiressl%2Cvprv%2Cmime%2Cgir%2Cclen%2Cdur%2Clmt&sig=AOq0QJ8wRgIhAIBPs4twuW1b7wQLDUWNl61A5WiNz5QOAgJsDLbc7mtcAiEAhx-bQvRG15Lv1gHe_la_uYNdcTlN_qZBZe6UMhAiSUE%3D&lsparams=mh%2Cmm%2Cmn%2Cms%2Cmv%2Cmvi%2Cpl%2Cinitcwndbps&lsig=AG3C_xAwRgIhAObaFfKag-QO7_55hp-r0n1qz20j-3csyCXSob30wuSOAiEAiV95Ies8k7ykXo0UozH1hv_-NRJH3oMg9D4LoQ-Uz5M%3D";
//
//     let video_resp = client.execute(client.get(video_url).build()?).await?;
//     let audio_resp = client.execute(client.get(audio_url).build()?).await?;
//
//     let video_stream = video_resp.bytes_stream().map_err(anyhow::Error::new);
//
//     let audio_stream = audio_resp.bytes_stream().map_err(anyhow::Error::new);
//
//     let mut output = tokio::fs::File::create("output.mp4").await?;
//
//     println!("Starting the remux...");
//
//     let out_stream = remuxer::remux(video_stream, audio_stream).await?;
//
//     pin!(out_stream);
//     while let Some(b) = out_stream.next().await {
//         let b = b?;
//
//         output.write_all(b.as_ref()).await?;
//     }
//
//     println!("DONE!!");
//
//     Ok(())
// }

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    init_tracing::init_tracing()?;

    let environment = std::env::var("ENVIRONMENT").context(
        "Please set ENVIRONMENT env var (probably you want to use either 'prod' or 'dev')",
    )?;

    let config = config::Config::load(&environment).context("Loading config has failed")?;

    info!("Resolved config: {:#?}", config);

    let client = connect_and_login(&config.telegram).await?;

    let dispatcher = DownloadDispatcher::new(vec![
        Arc::new(YoutubeDownloader::new()),
        Arc::new(TikTokDownloader::new()),
    ]);
    let dispatcher = Arc::new(dispatcher);

    tokio::select!(
        _ = tokio::signal::ctrl_c() => {
            info!("Got SIGINT; quitting early gracefully");
        }
        r = bot::run_bot(&client, dispatcher, Duration::from_secs(5)) => {
            match r {
                Ok(_) => info!("Got disconnected from Telegram gracefully"),
                Err(e) => error!("Error during update handling: {}", e),
            }
        }
        r = save_session_periodic(&client, &config.telegram) => {
            match r {
                Ok(_) => unreachable!(),
                Err(e) => error!("Error during session saving: {}", e),
            }
        }
    );

    save_session(&client, &config.telegram)?;

    Ok(())
}
