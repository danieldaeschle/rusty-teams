use browser::Browser;

#[tokio::main]
async fn main() {
    let browser = match Browser::detect() {
        Ok(browser) => browser,
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    };
    let config = browser.config();
    println!("platform: {:?}, port: {}", config.platform, config.port);
    match browser.status().await {
        Ok(status) if !status.running => println!("Chrome not running"),
        Ok(status) => {
            println!("Chrome: {:?}, login: {:?}", status.mode, status.login_state);
            println!("renderer: {}", browser.probe_renderer().await.label());
            for tab in status.tabs {
                let app = tab.app.map_or("-", |app| app.name());
                let parked = if tab.parked { " (parked)" } else { "" };
                println!("  tab {app}: {}{parked}", tab.host.as_deref().unwrap_or("(no host)"));
            }
        }
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}
