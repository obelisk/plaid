use crossbeam_channel::bounded;
use plaid::apis::github::Authentication;
use serde::Deserialize;

use plaid::data::{get_and_process_dg_logs, github::*};

use std::env;
use std::time::Duration;

#[derive(Deserialize)]
struct GitHubLog<'a> {
    #[serde(rename = "@timestamp")]
    timestamp: u64,
    action: &'a str,
    actor: Option<&'a str>,
    permission: Option<&'a str>,
    new_repo_permission: Option<&'a str>,
}

impl std::fmt::Display for GitHubLog<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}\t{}\t{:?}\t{:?}\t{:?}",
            self.timestamp, self.action, self.actor, self.permission, self.new_repo_permission
        )
    }
}

fn get_config_from_env() -> Result<GithubConfig, ()> {
    let token = match env::var("GH_TOKEN") {
        Ok(x) => x,
        Err(_) => {
            println!("Need to define GH_TOKEN to tail GitHub audit logs");
            return Err(());
        }
    };

    let authentication = Authentication::Token { token };

    let org = match env::var("GH_ORG") {
        Ok(x) => x,
        Err(_) => {
            println!("Need to define GH_TOKEN to tail GitHub audit logs");
            return Err(());
        }
    };

    Ok(GithubConfig::new(authentication, org, LogType::Web))
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let config = get_config_from_env().unwrap();

    let (logger_tx, logger_rx) = bounded(2048);

    let mut gh = Github::new(config, logger_tx);

    loop {
        //println!("Start of log group");
        get_and_process_dg_logs(&mut gh, None).await.unwrap();

        while let Ok(log) = logger_rx.recv_timeout(Duration::from_secs(0)) {
            let log: GitHubLog = serde_json::from_slice(&log.data).unwrap();
            println!("{}", log);
        }
        //println!("End of log group");
        tokio::time::sleep(Duration::from_secs(6)).await;
    }
}
