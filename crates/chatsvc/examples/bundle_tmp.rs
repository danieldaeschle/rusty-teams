use serde_json::json;
use session::{App, DEFAULT_ENDPOINT, Scope, Session};

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let out = std::env::args().nth(1).expect("out dir");
    let pattern = std::env::args().nth(2).expect("pattern");
    let session = Session::connect(&endpoint).await.expect("browser");
    let body = r#"
const found = {};
for (const name of await caches.keys()) {
  const cache = await caches.open(name);
  for (const request of await cache.keys()) {
    if (!new RegExp(args.pattern).test(request.url)) continue;
    const response = await cache.match(request);
    found[request.url] = await response.text();
  }
}
const scripts = performance.getEntriesByType('resource').map(entry => entry.name).filter(url => new RegExp(args.pattern).test(url));
for (const url of scripts) { if (!found[url]) { try { found[url] = await (await fetch(url)).text(); } catch (error) {} } }
return found;
"#;
    let scope = Scope::new("https://substrate.office.com", "SubstrateSearch-Internal.ReadWrite");
    let found = session.run_with_token(App::Teams, &scope, body, &json!({"pattern": pattern}), false).await.expect("eval");
    for (url, text) in found.as_object().unwrap() {
        let name = url.rsplit('/').next().unwrap();
        std::fs::write(format!("{out}/{name}"), text.as_str().unwrap()).unwrap();
        println!("{url} {}", text.as_str().unwrap().len());
    }
}
