//! Dusk's scheduler. It switches only when the schedule's answer changes, so
//! a manual switch in between is respected until the next sunrise/sunset.

use std::time::Duration;

use forgekit_core::Capabilities;
use forgekit_core::settings::General;
use forgekit_core::tools::dusk::{self, Mode, Settings};

pub async fn run() {
    let desktop = tokio::task::spawn_blocking(|| Capabilities::detect().desktop)
        .await
        .unwrap_or(forgekit_core::capability::Desktop::Other);
    let mut last: Option<Mode> = None;
    loop {
        let enabled = General::load().unwrap_or_default().is_enabled("dusk");
        let settings = Settings::load().unwrap_or_default();
        match dusk::desired(&settings.schedule, chrono::Local::now()) {
            Ok(Some(mode)) if enabled && last != Some(mode) => {
                let s = settings.clone();
                let applied = tokio::task::spawn_blocking(move || {
                    if dusk::current(desktop).ok() != Some(mode) { dusk::apply(mode, desktop, &s) } else { Ok(()) }
                })
                .await;
                match applied {
                    Ok(Ok(())) => last = Some(mode),
                    Ok(Err(e)) => eprintln!("dusk: {e:#}"),
                    Err(e) => eprintln!("dusk: {e}"),
                }
            }
            Ok(None) => last = None,
            Ok(Some(_)) => {}
            Err(e) => eprintln!("dusk: {e:#}"),
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}
